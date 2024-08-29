mod cache;
mod config;
mod constant;

use crate::cache::Cache;
use crate::constant::{
  CONFIG_NOT_FOUND_2, DATA_ID_NOT_FOUND_1, DATA_ID_NOT_FOUND_2, GROUP_NOT_FOUND_1,
  GROUP_NOT_FOUND_2,
};
use axum::{
  body::Body,
  extract::Query,
  http::{HeaderMap, Request, StatusCode},
  routing::{any, get, post},
  Router,
};
use lambda_extension::{
  service_fn,
  tracing::{self, error},
  Error, LambdaEvent, NextEvent,
};
use serde_json::json;
use std::{
  collections::{HashMap, HashSet},
  env,
  net::{Ipv4Addr, SocketAddrV4},
  sync::Arc,
  time::Duration,
};
use tokio::{
  net::TcpListener,
  sync::{broadcast, mpsc},
  time::sleep,
};

async fn start_nacos_adapter(mut refresh_rx: mpsc::Receiver<()>) {
  let port = env::var("AWS_LAMBDA_NACOS_ADAPTER_PORT")
    .ok()
    .and_then(|p| p.parse().ok())
    .unwrap_or(8848);
  let prefix = Arc::new(
    env::var("AWS_LAMBDA_NACOS_ADAPTER_CONFIG_PATH").unwrap_or("/mnt/efs/nacos/".to_string()),
  );
  let cache_size = env::var("AWS_LAMBDA_NACOS_ADAPTER_CACHE_SIZE")
    .ok()
    .and_then(|s| s.parse().ok())
    .unwrap_or(64);

  let cache = Cache::new(cache_size);

  let (target_tx, mut target_rx) = mpsc::channel::<String>(1);
  let (config_tx, _) = broadcast::channel(1);

  tokio::spawn({
    let mut cache = cache.clone();
    let config_tx = config_tx.clone();
    async move {
      let mut targets = HashSet::new();
      loop {
        tokio::select! {
          target = target_rx.recv() => {
            targets.insert(target.unwrap());
          }
          _ = refresh_rx.recv() => {
            for t in &targets {
              if let Ok(config) = cache.get(t.clone()).await {
                config_tx.send((t.clone(), config)).unwrap();
              }
            }
          }
        }
      }
    }
  });

  macro_rules! handle_get_config {
    ($prefix:expr, $tenant:expr, $group:expr, $data_id:expr, $cache:expr) => {{
      let path = format!("{}{}/{}/{}", $prefix, $tenant, $group, $data_id);

      match $cache.get(path.clone()).await {
        Ok(config) => Some(config.content.clone()),
        Err(e) => {
          error!(path, error = %e.to_string(), "failed to get config");
          None
        }
      }
    }};
  }

  let app = Router::new()
    .route(
      "/nacos/v1/cs/configs",
      get({
        let prefix = prefix.clone();
        let mut cache = cache.clone();
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
            .map(|s| s as &str)
            .unwrap_or("public");

          match handle_get_config!(prefix, tenant, group, data_id, cache) {
            Some(config) => (StatusCode::OK, config),
            None => (StatusCode::NOT_FOUND, "Not Found".to_string()),
          }
        }
      }),
    )
    .route(
      "/nacos/v2/cs/config",
      get({
        let prefix = prefix.clone();
        let mut cache = cache.clone();
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
            .map(|s| s as &str)
            .unwrap_or("public");

          match handle_get_config!(prefix, tenant, group, data_id, cache) {
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
        let prefix = prefix.clone();
        let mut cache = cache.clone();
        let config_tx = config_tx.clone();
        async move {
          if !body.starts_with("Listening-Configs=") {
            // TODO: checkout the actually nacos response
            return (StatusCode::BAD_REQUEST, "Bad Request".to_string());
          }

          let mut update_now = vec![];
          let mut map = HashMap::new();
          for (target, md5, raw) in body["Listening-Configs=".len()..].split("%01").map(|s| {
            let mut parts = s.split("%02");
            let data_id = parts.next().unwrap();
            let group = parts.next().unwrap();
            let md5 = parts.next().unwrap();
            let tenant = parts.next().unwrap_or("public");
            let path = format!("{}{}/{}/{}", prefix, tenant, group, data_id);
            (path, md5, s)
          }) {
            // TODO: do this in parallel
            target_tx.send(target.clone()).await.unwrap();
            if md5 != cache.get(target.clone()).await.unwrap().md5 {
              update_now.push(raw);
            }
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
                if let Ok((path, config)) = res {
                  let (md5, raw) = map.get(&path).unwrap();
                  if md5 != &config.md5 {
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

#[tokio::main]
async fn main() {
  tracing::init_default_subscriber();

  let (refresh_tx, refresh_rx) = mpsc::channel(1);

  tokio::spawn(start_nacos_adapter(refresh_rx));

  lambda_extension::run(service_fn(move |event: LambdaEvent| {
    let refresh_tx = refresh_tx.clone();
    async move {
      match event.next {
        NextEvent::Shutdown(_e) => {}
        NextEvent::Invoke(_e) => {
          refresh_tx.send(()).await.unwrap();
          // TODO: wait for refresh done?
        }
      }
      Ok(()) as Result<(), Error>
    }
  }))
  .await
  .unwrap()
}
