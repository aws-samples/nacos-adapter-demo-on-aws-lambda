use super::provider::ConfigProvider;
use futures::future::join_all;
use lambda_extension::tracing::{debug, trace};
use std::{
  collections::{hash_map::Entry, HashMap},
  sync::Arc,
};
use tokio::sync::{broadcast, mpsc};

/// This is cheap to clone. This is `Send` and `Sync`.
#[derive(Debug, Hash, PartialEq, Eq, Clone)]
pub struct Target {
  pub data_id: Arc<String>,
  pub group: Arc<String>,
  pub tenant: Option<Arc<String>>,
}

impl Target {
  /// Returns `"{data_id}\x02{group}\x02{tenant}\x01"`.
  pub fn to_param_string(&self) -> String {
    format!(
      "{}\x02{}\x02{}\x01",
      self.data_id,
      self.group,
      self.tenant().unwrap_or("")
    )
  }

  pub fn tenant(&self) -> Option<&str> {
    self.tenant.as_deref().map(|s| s.as_str())
  }
}

struct TargetState {
  client_md5: String,
  latest_md5: String,
  changed_tx: Option<mpsc::Sender<()>>,
}

pub fn spawn_target_manager(
  cp: impl ConfigProvider + 'static,
  mut refresh_rx: mpsc::Receiver<mpsc::Sender<()>>,
) -> (mpsc::Sender<(Target, String)>, broadcast::Sender<Target>) {
  // this channel is used to register listening targets to the target manager
  let (target_tx, mut target_rx) = mpsc::channel::<(Target, String)>(1);
  // this channel is used to send updated target from the target manager to the long connection
  let (config_tx, _) = broadcast::channel(1);

  // spawn the target manager
  tokio::spawn({
    let config_tx = config_tx.clone();
    async move {
      let mut targets = HashMap::new();
      loop {
        tokio::select! {
          target = target_rx.recv() => {
            debug!("register target: {:?}", target); // target might be None
            let Some((target, md5)) = target else { break };
            match targets.entry(target) {
              Entry::Vacant(entry) => {
                entry.insert(TargetState {
                  client_md5: md5.clone(),
                  latest_md5: md5,
                  changed_tx: None,
                });
              },
              Entry::Occupied(mut entry) => {
                let state = entry.get_mut();
                if md5 == state.latest_md5 {
                  // client md5 matches the latest md5,
                  // take and drop the sender to mark the refresh as done
                  state.changed_tx.take();
                }
                state.client_md5 = md5;
              },
            }
          }
          changed_tx = refresh_rx.recv() => {
            debug!("refreshing all targets: {:?}", changed_tx.is_some());
            let Some(changed_tx) = changed_tx else { break };

            join_all(targets.iter_mut().map(|(target, state)| {
              let mut cp = cp.clone();
              let config_tx = config_tx.clone();
              let changed_tx = changed_tx.clone();
              async move {
                if let Ok(config) = cp.get(&target.data_id, &target.group, target.tenant(), true).await {
                  let new_md5 = config.md5();
                  let client_md5 = &state.client_md5;
                  if new_md5 != client_md5 {
                    debug!(client_md5, new_md5, "md5 mismatch");
                    state.latest_md5 = new_md5.to_owned();
                    changed_tx.send(()).await.expect("changed_tx.send failed");
                    state.changed_tx = Some(changed_tx.clone());
                    // it's ok if the config_tx.send failed
                    // it means the long connection is disconnected but might be reconnected later
                    if config_tx.send(target.clone()).is_err() {
                      debug!("config_tx.send failed, which means no long connection is listening");
                    }
                  }
                }
              }
            })).await;
          }
        }
      }
      trace!("target manager is stopped");
    }
  });

  (target_tx, config_tx)
}
