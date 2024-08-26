mod constant;

use crate::constant::{
  CONFIG_NOT_FOUND_2, DATA_ID_NOT_FOUND_1, DATA_ID_NOT_FOUND_2, GROUP_NOT_FOUND_1,
  GROUP_NOT_FOUND_2,
};
use axum::{
  body::Body,
  extract::Query,
  http::{Request, StatusCode},
  routing::{any, get},
  Router,
};
use lambda_extension::{
  tracing::{self, error},
  Extension,
};
use serde_json::json;
use shinkuro::Cache;
use std::{
  collections::HashMap,
  env,
  net::{Ipv4Addr, SocketAddrV4},
  sync::Arc,
};
use tokio::net::TcpListener;

async fn start_nacos_adapter() {
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

  let mut cache = Cache::new(cache_size);

  macro_rules! handle_get_config {
    ($prefix:expr, $tenant:expr, $group:expr, $data_id:expr, $cache:expr) => {{
      let path = format!("{}{}/{}/{}", $prefix, $tenant, $group, $data_id);

      match $cache.get(path.clone()).await {
        Ok(config) => Some((*config).clone()),
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
                "data": *config
              })
              .to_string(),
            ),
            None => (StatusCode::NOT_FOUND, CONFIG_NOT_FOUND_2.to_string()),
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

  tokio::spawn(start_nacos_adapter());

  Extension::new().run().await.unwrap()
}
