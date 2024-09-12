use crate::{
  config::provider::ConfigProvider,
  constant::{
    CONFIG_NOT_FOUND_2, DATA_ID_NOT_FOUND_1, DATA_ID_NOT_FOUND_2, GROUP_NOT_FOUND_1,
    GROUP_NOT_FOUND_2,
  },
};
use axum::{
  body::Body,
  extract::Query,
  http::{HeaderMap, Request, StatusCode},
  routing::{any, get, post},
  Form, Router,
};
use futures::future::join_all;
use lambda_extension::tracing::{error, trace};
use serde::Deserialize;
use serde_json::json;
use std::{
  collections::{HashMap, HashSet},
  sync::Arc,
  time::Duration,
};
use tokio::{
  net::TcpListener,
  sync::{broadcast, mpsc},
  time::sleep,
};

#[derive(Debug, Hash, PartialEq, Eq)]
struct Target {
  pub data_id: String,
  pub group: String,
  pub tenant: Option<String>,
}

#[derive(Deserialize)]
struct ListeningConfig {
  #[serde(rename = "Listening-Configs")]
  targets: String,
}

pub async fn start_nacos_adapter(
  listener: TcpListener,
  mut refresh_rx: mpsc::Receiver<mpsc::Sender<()>>,
  cp: impl ConfigProvider + Clone + Send + 'static,
) {
  // this channel is used to register listening targets to the target manager
  let (target_tx, mut target_rx) = mpsc::channel::<Arc<Target>>(1);
  // this channel is used to send updated target from the target manager to the long connection
  let (config_tx, _) = broadcast::channel(1);

  // spawn the target manager
  tokio::spawn({
    let cp = cp.clone();
    let config_tx = config_tx.clone();
    async move {
      let mut targets = HashSet::new();
      loop {
        tokio::select! {
          target = target_rx.recv() => {
            trace!("register target: {:?}", target.as_ref().map(|t| t.as_ref()));
            let Some(target) = target else { break };
            targets.insert(target);
          }
          changed_tx = refresh_rx.recv() => {
            trace!("refreshing all targets: {:?}", changed_tx.is_some());
            let Some(changed_tx) = changed_tx else { break };

            join_all(targets.iter().map(|target| {
              let mut cp = cp.clone();
              let config_tx = config_tx.clone();
              let changed_tx = changed_tx.clone();
              async move {
                if let Ok(config) = cp.get(&target.data_id, &target.group, target.tenant.as_deref()).await {
                  // it's ok if the config_tx.send failed
                  // it means the long connection is disconnected but might be reconnected later
                  config_tx.send((target.clone(), config, changed_tx.clone())).ok();
                }
              }
            })).await;
          }
        }
      }
      trace!("target manager is stopped");
    }
  });

  macro_rules! handle_get_config {
    ($data_id:expr, $group:expr, $tenant:expr, $cp:expr) => {{
      match $cp.get($data_id, $group, $tenant).await {
        Ok(config) => Some(config.content().to_owned()),
        Err(e) => {
          let data_id = $data_id;
          let group = $group;
          let tenant = $tenant;
          error!(data_id, group, tenant, error = %e.to_string(), "failed to get config");
          None
        }
      }
    }};
  }

  let app = Router::new()
    .route(
      "/nacos/v1/cs/configs",
      get({
        let mut cp = cp.clone();
        move |Query(params): Query<HashMap<String, String>>| async move {
          let Some(data_id) = get_non_empty(&params, "dataId") else {
            return (
              StatusCode::INTERNAL_SERVER_ERROR,
              DATA_ID_NOT_FOUND_1.to_string(),
            );
          };
          let Some(group) = get_non_empty(&params, "group") else {
            return (
              StatusCode::INTERNAL_SERVER_ERROR,
              GROUP_NOT_FOUND_1.to_string(),
            );
          };
          let tenant = get_non_empty(&params, "tenant").map(|s| s as &str);

          match handle_get_config!(data_id, group, tenant, cp) {
            Some(config) => (StatusCode::OK, config.to_string()),
            None => (StatusCode::NOT_FOUND, "Not Found".to_string()),
          }
        }
      }),
    )
    .route(
      "/nacos/v2/cs/config",
      get({
        let mut cp = cp.clone();
        move |Query(params): Query<HashMap<String, String>>| async move {
          let Some(data_id) = get_non_empty(&params, "dataId") else {
            return (StatusCode::BAD_REQUEST, DATA_ID_NOT_FOUND_2.to_string());
          };
          let Some(group) = get_non_empty(&params, "group") else {
            return (StatusCode::BAD_REQUEST, GROUP_NOT_FOUND_2.to_string());
          };
          let tenant = get_non_empty(&params, "namespaceId").map(|s| s as &str);

          // TODO: "tag" in nacos api v2 is not supported yet
          // TODO: grpc in nacos api v2 is not supported yet

          match handle_get_config!(data_id, group, tenant, cp) {
            Some(config) => (
              StatusCode::OK,
              json!({
                "code": 0,
                "message": "success",
                "data": config
              })
              .to_string(),
            ),
            None => (StatusCode::NOT_FOUND, CONFIG_NOT_FOUND_2.to_string()),
          }
        }
      }),
    )
    .route(
      "/nacos/v1/cs/configs/listener",
      post(
        move |headers: HeaderMap, Form(ListeningConfig { targets })| {
          trace!(targets, "listening config");
          let mut cp = cp.clone();
          let mut config_rx = config_tx.subscribe();
          async move {
            if targets.is_empty() {
              // TODO: checkout the real response of nacos
              return (
                StatusCode::BAD_REQUEST,
                "missing listening config".to_string(),
              );
            }

            let mut update_now = vec![];
            // target => md5
            let mut target_map = HashMap::new();
            for (target, md5) in targets.split('\x01').filter(|s| s.len() > 0).map(|s| {
              let mut parts = s.split('\x02');
              // TODO: better error handling
              let data_id = parts.next().unwrap();
              let group = parts.next().unwrap();
              let md5 = parts.next().unwrap();
              let tenant = parts.next();
              (
                Arc::new(Target {
                  data_id: data_id.to_string(),
                  group: group.to_string(),
                  tenant: tenant.map(|s| s.to_string()),
                }),
                md5,
              )
            }) {
              // TODO: do this in parallel
              target_tx.send(target.clone()).await.unwrap();
              let cached = cp
                .get(
                  &target.data_id,
                  &target.group,
                  target.tenant.as_ref().map(|s| s as &str),
                )
                .await
                .unwrap();
              let cached_md5 = cached.md5();
              if md5 != cached_md5 {
                trace!(md5, cached_md5, "md5 not match");
                update_now.push(target.clone());
              }
              target_map.insert(target, md5);
            }

            if !update_now.is_empty() {
              let res = update_now
                .iter()
                .map(|t| {
                  format!(
                    "{}%02{}%02{}",
                    t.data_id,
                    t.group,
                    t.tenant.as_ref().map(|s| s as &str).unwrap_or("")
                  )
                })
                .collect::<Vec<_>>()
                .join("%01")
                + "%01";
              trace!(res, "immediate update");
              return (StatusCode::OK, res);
            }

            let timeout = headers
              .get("Long-Pulling-Timeout")
              .and_then(|s| s.to_str().ok())
              .and_then(|s| s.parse().ok())
              .unwrap_or(30000);
            let timeout = sleep(Duration::from_millis(timeout));
            tokio::pin!(timeout);

            loop {
              tokio::select! {
                _ = &mut timeout => {
                  // timeout, nothing is changed
                  trace!("listener timeout");
                  return (StatusCode::OK, "".to_string())
                }
                res = config_rx.recv() => {
                  if let Ok((target, config, changed_tx)) = res {
                    let md5 = target_map.get(&target).unwrap();
                    let new_md5 = config.md5();
                    if md5 != &config.md5() {
                      trace!(md5, new_md5, "md5 not match");
                      // TODO: optimize code, add a method to Target, use correct url encoding
                      let res = format!(
                        "{}%02{}%02{}",
                        target.data_id,
                        target.group,
                        target.tenant.as_ref().map(|s| s as &str).unwrap_or("")
                      ) + "%01";
                      trace!(res, "update");
                      changed_tx.send(()).await.expect("changed_rx should not be dropped");
                      return (StatusCode::OK, res);
                    }
                  }
                }
              }
            }
          }
        },
      ),
    )
    .fallback(any(|request: Request<Body>| async move {
      error!(uri = %request.uri().to_string(), "unhandled request");
      (StatusCode::NOT_FOUND, "Not Found".to_string())
    }));

  axum::serve(listener, app).await.unwrap();
}

fn get_non_empty<'a>(params: &'a HashMap<String, String>, key: &str) -> Option<&'a String> {
  params.get(key).filter(|s| s.len() > 0)
}
