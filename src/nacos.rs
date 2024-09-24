use crate::{
  config::{provider::ConfigProvider, target::Target, Config},
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
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::{
  net::TcpListener,
  sync::{broadcast, mpsc, Mutex},
  time::sleep,
};
use urlencoding::encode;

#[derive(Deserialize)]
struct ListeningConfig {
  #[serde(rename = "Listening-Configs")]
  targets: String,
}

pub async fn start_nacos_adapter(
  listener: TcpListener,
  target_tx: mpsc::Sender<(Arc<Target>, String)>,
  config_tx: broadcast::Sender<(Arc<Target>, Arc<Config>, mpsc::Sender<()>)>,
  cp: impl ConfigProvider + Clone + Send + 'static,
) {
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
          let cp = cp.clone();
          let mut config_rx = config_tx.subscribe();
          async move {
            if targets.is_empty() {
              return (
                StatusCode::BAD_REQUEST,
                "caused: invalid probeModify;".to_string(),
              );
            }

            let update_now = Arc::new(Mutex::new(vec![]));
            join_all(targets.split('\x01').filter(|s| !s.is_empty()).map(|s| {
              let mut parts = s.split('\x02');
              // TODO: better error handling
              let data_id = parts.next().unwrap();
              let group = parts.next().unwrap();
              let md5 = parts.next().unwrap();
              let tenant = parts.next();
              let target = Arc::new(Target {
                data_id: data_id.to_string(),
                group: group.to_string(),
                tenant: tenant.map(|s| s.to_string()),
              });
              let mut cp = cp.clone();
              let target_tx = target_tx.clone();
              let update_now = update_now.clone();
              async move {
                target_tx
                  .send((target.clone(), md5.to_owned()))
                  .await
                  .unwrap();
                let cached = cp
                  .get(&target.data_id, &target.group, target.tenant.as_deref())
                  .await
                  .unwrap();
                let cached_md5 = cached.md5();
                if md5 != cached_md5 {
                  trace!(md5, cached_md5, "md5 not match");
                  update_now.lock().await.push(target);
                }
              }
            }))
            .await;

            let update_now = Arc::try_unwrap(update_now).unwrap().into_inner();
            if !update_now.is_empty() {
              let res = encode(
                &update_now
                  .iter()
                  .map(|t| t.to_string())
                  .collect::<Vec<_>>()
                  .join(""),
              )
              .to_string();
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
                  if let Ok((target, _config, changed_tx)) = res {
                    let res = encode(&target.to_string()).to_string();
                    trace!(res, "update");
                    changed_tx.send(()).await.expect("changed_rx should not be dropped");
                    return (StatusCode::OK, res);
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
  params.get(key).filter(|s| !s.is_empty())
}
