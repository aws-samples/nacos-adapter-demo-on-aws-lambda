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
  Router,
};
use lambda_extension::tracing::error;
use serde_json::json;
use std::{
  collections::{HashMap, HashSet},
  net::{Ipv4Addr, SocketAddrV4},
  time::Duration,
};
use tokio::{
  net::TcpListener,
  sync::{broadcast, mpsc},
  time::sleep,
};

pub async fn start_nacos_adapter(
  port: u16,
  mut refresh_rx: mpsc::Receiver<()>,
  cp: impl ConfigProvider + Clone + Send + 'static,
) {
  let (target_tx, mut target_rx) = mpsc::channel::<(String, String, Option<String>)>(1);
  let (config_tx, _) = broadcast::channel(1);

  tokio::spawn({
    let mut cp = cp.clone();
    let config_tx = config_tx.clone();
    async move {
      let mut targets = HashSet::new();
      loop {
        tokio::select! {
          target = target_rx.recv() => {
            targets.insert(target.unwrap());
          }
          _ = refresh_rx.recv() => {
            for (data_id, group, tenant) in &targets {
              if let Ok(config) = cp.get(data_id, group, tenant.as_ref().map(|s| s as &str)).await {
                config_tx.send(((data_id.to_string(), group.to_string(), tenant.clone()), config)).unwrap();
              }
            }
          }
        }
      }
    }
  });

  macro_rules! handle_get_config {
    ($tenant:expr, $group:expr, $data_id:expr, $cp:expr) => {{
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
          let Some(data_id) = params.get("dataId").filter(|s| s.len() > 0) else {
            return (
              StatusCode::INTERNAL_SERVER_ERROR,
              DATA_ID_NOT_FOUND_1.to_string(),
            );
          };
          let Some(group) = params.get("group").filter(|s| s.len() > 0) else {
            return (
              StatusCode::INTERNAL_SERVER_ERROR,
              GROUP_NOT_FOUND_1.to_string(),
            );
          };

          let tenant = params
            .get("tenant")
            .filter(|s| s.len() > 0)
            .map(|s| s as &str);

          match handle_get_config!(tenant, group, data_id, cp) {
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
          let Some(data_id) = params.get("dataId").filter(|s| s.len() > 0) else {
            return (StatusCode::BAD_REQUEST, DATA_ID_NOT_FOUND_2.to_string());
          };
          let Some(group) = params.get("group").filter(|s| s.len() > 0) else {
            return (StatusCode::BAD_REQUEST, GROUP_NOT_FOUND_2.to_string());
          };

          // TODO: "tag" in nacos api v2 is not supported yet

          let tenant = params
            .get("namespaceId")
            .filter(|s| s.len() > 0)
            .map(|s| s as &str);

          match handle_get_config!(tenant, group, data_id, cp) {
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
      post(move |headers: HeaderMap, body: String| {
        let mut cp = cp.clone();
        let config_tx = config_tx.clone();
        async move {
          if !body.starts_with("Listening-Configs=") {
            // TODO: checkout the actually nacos response
            return (StatusCode::BAD_REQUEST, "Bad Request".to_string());
          }

          let mut update_now = vec![];
          let mut map = HashMap::new();
          for (data_id, group, tenant, md5, raw) in
            body["Listening-Configs=".len()..].split("%01").map(|s| {
              let mut parts = s.split("%02");
              let data_id = parts.next().unwrap();
              let group = parts.next().unwrap();
              let md5 = parts.next().unwrap();
              let tenant = parts.next();
              (data_id, group, tenant, md5, s)
            })
          {
            // TODO: do this in parallel
            target_tx
              .send((
                data_id.to_string(),
                group.to_string(),
                tenant.map(|s| s.to_string()),
              ))
              .await
              .unwrap();
            if md5 != cp.get(data_id, group, tenant).await.unwrap().md5() {
              update_now.push(raw);
            }
            let target = format!("{}/{}/{}", tenant.unwrap_or("public"), group, data_id);
            map.insert(target, (md5, raw));
          }

          if !update_now.is_empty() {
            return (StatusCode::OK, update_now.join("%01"));
          }

          let timeout = headers
            .get("Long-Pulling-Timeout")
            .and_then(|s| s.to_str().ok())
            .and_then(|s| s.parse().ok())
            .unwrap_or(30000);
          let timeout = sleep(Duration::from_millis(timeout));
          tokio::pin!(timeout);
          let mut config_rx = config_tx.subscribe();

          loop {
            tokio::select! {
              _ = &mut timeout => {
                // timeout, nothing is changed
                return (StatusCode::OK, "".to_string())
              }
              res = config_rx.recv() => {
                if let Ok(((data_id, group, tenant), config)) = res {
                  let (md5, raw) = map.get(&format!("{}/{}/{}", tenant.as_ref().map(|s| s as &str).unwrap_or("public"), group, data_id)).unwrap();
                  if md5 != &config.md5() {
                    return (StatusCode::OK, raw.to_string())
                  }
                }
              }
            }
          }
        }
      }),
    )
    .fallback(any(|request: Request<Body>| async move {
      error!(uri = %request.uri().to_string(), "unhandled request");
      (StatusCode::NOT_FOUND, "Not Found".to_string())
    }));

  let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), port))
    .await
    .unwrap();
  axum::serve(listener, app).await.unwrap();
}
