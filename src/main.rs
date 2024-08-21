use axum::{extract::Query, http::StatusCode, routing::get, Router};
use lambda_extension::{tracing, Extension};
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

  let cache = Cache::new(cache_size);

  let config_getter = |ns: &'static str| {
    let prefix = prefix.clone();
    let mut cache = cache.clone();
    move |Query(params): Query<HashMap<String, String>>| async move {
      let Some(group) = params.get("group").filter(|s| s.len() > 0) else {
        return (StatusCode::BAD_REQUEST, "group is required".to_string());
      };
      let Some(data_id) = params.get("dataId").filter(|s| s.len() > 0) else {
        return (StatusCode::BAD_REQUEST, "dataId is required".to_string());
      };

      // TODO: "tag" in nacos api v2 is not supported yet

      let tenant = params
        .get(ns)
        .filter(|s| s.len() > 0)
        .map(|s| s as &str)
        .unwrap_or("public");
      let path = format!("{}{}/{}/{}", prefix, tenant, group, data_id);

      match cache.get(path).await {
        Ok(config) => (StatusCode::OK, (*config).clone()),
        Err(e) => (StatusCode::NOT_FOUND, e.to_string()),
      }
    }
  };

  let app = Router::new()
    .route("/nacos/v1/cs/configs", get(config_getter("tenant")))
    .route("/nacos/v2/cs/config", get(config_getter("namespaceId")));

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
