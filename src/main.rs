mod config;
mod constant;
mod nacos;

use crate::{
  config::{fs::FsConfigProvider, proxy::ProxyConfigProvider},
  nacos::start_nacos_adapter,
};
use lambda_extension::{service_fn, tracing, Error, LambdaEvent, NextEvent};
use std::env;
use tokio::sync::mpsc;

#[tokio::main]
async fn main() {
  tracing::init_default_subscriber();

  let (refresh_tx, refresh_rx) = mpsc::channel(1);

  let cache_size = env::var("AWS_LAMBDA_NACOS_ADAPTER_CACHE_SIZE")
    .ok()
    .and_then(|s| s.parse().ok())
    .unwrap_or(64);

  if let Ok(origin) = env::var("AWS_LAMBDA_NACOS_ADAPTER_ORIGIN_ADDRESS") {
    tokio::spawn(start_nacos_adapter(
      refresh_rx,
      ProxyConfigProvider::new(cache_size, origin),
    ));
  } else {
    let prefix =
      env::var("AWS_LAMBDA_NACOS_ADAPTER_CONFIG_PATH").unwrap_or("/mnt/efs/nacos/".to_string());

    tokio::spawn(start_nacos_adapter(
      refresh_rx,
      FsConfigProvider::new(cache_size, prefix),
    ));
  }

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
