use axum::{extract::Query, routing::get, Router};
use lambda_extension::{tracing, Extension};
use std::{
  collections::HashMap,
  env,
  net::{Ipv4Addr, SocketAddrV4},
};
use tokio::{fs, net::TcpListener};

async fn start_nacos_adapter() {
  let port = env::var("AWS_LAMBDA_NACOS_ADAPTER_PORT")
    .ok()
    .and_then(|p| p.parse().ok())
    .unwrap_or(8848);
  let prefix =
    env::var("AWS_LAMBDA_NACOS_ADAPTER_CONFIG_PATH").unwrap_or("/mnt/efs/nacos".to_string());

  let app = Router::new().route(
    "/nacos/v1/cs/configs",
    get(move |Query(params): Query<HashMap<String, String>>| {
      let group = params.get("group").unwrap();
      let data_id = params.get("dataId").unwrap();
      let path = format!("{}/{}/{}", prefix, group, data_id);
      async move { fs::read_to_string(path).await.unwrap() }
    }),
  );

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
