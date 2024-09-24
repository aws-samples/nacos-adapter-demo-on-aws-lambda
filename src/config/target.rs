use super::provider::ConfigProvider;
use futures::future::join_all;
use lambda_extension::tracing::trace;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{broadcast, mpsc};

#[derive(Debug, Hash, PartialEq, Eq)]
pub struct Target {
  pub data_id: String,
  pub group: String,
  pub tenant: Option<String>,
}

impl Target {
  /// Returns `"{data_id}\x02{group}\x02{tenant}\x01"`.
  pub fn to_param_string(&self) -> String {
    format!(
      "{}\x02{}\x02{}\x01",
      self.data_id,
      self.group,
      self.tenant.as_deref().unwrap_or("")
    )
  }
}

pub fn spawn_target_manager(
  cp: impl ConfigProvider + Clone + Send + 'static,
  mut refresh_rx: mpsc::Receiver<mpsc::Sender<()>>,
) -> (
  mpsc::Sender<(Arc<Target>, String)>,
  broadcast::Sender<(Arc<Target>, mpsc::Sender<()>)>,
) {
  // this channel is used to register listening targets to the target manager
  let (target_tx, mut target_rx) = mpsc::channel::<(Arc<Target>, String)>(1);
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
            trace!("register target: {:?}", target.as_ref().map(|(target, md5)| (target.as_ref(), md5)));
            let Some((target, md5)) = target else { break };
            targets.insert(target, md5);
          }
          changed_tx = refresh_rx.recv() => {
            trace!("refreshing all targets: {:?}", changed_tx.is_some());
            let Some(changed_tx) = changed_tx else { break };

            join_all(targets.iter().map(|(target, md5)| {
              let mut cp = cp.clone();
              let config_tx = config_tx.clone();
              let changed_tx = changed_tx.clone();
              async move {
                if let Ok(config) = cp.get(&target.data_id, &target.group, target.tenant.as_deref()).await {
                  let new_md5 = config.md5();
                  if new_md5 != md5 {
                    trace!(md5, new_md5, "md5 not match");
                    // it's ok if the config_tx.send failed
                    // it means the long connection is disconnected but might be reconnected later
                    config_tx.send((target.clone(), changed_tx.clone())).ok();
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
