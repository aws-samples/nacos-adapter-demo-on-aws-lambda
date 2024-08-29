mod config;
mod constant;
mod nacos;

use crate::{
  config::{fs::FsConfigProvider, proxy::ProxyConfigProvider},
  nacos::start_nacos_adapter,
};
use lambda_extension::{
  service_fn,
  tracing::{self, debug, trace},
  Error, LambdaEvent, NextEvent,
};
use std::{
  env,
  net::{Ipv4Addr, SocketAddrV4},
};
use tokio::{net::TcpListener, sync::mpsc};

#[tokio::main]
async fn main() -> Result<(), Error> {
  tracing::init_default_subscriber();

  let port = env::var("AWS_LAMBDA_NACOS_ADAPTER_PORT")
    .ok()
    .and_then(|p| p.parse().ok())
    .unwrap_or(8848);
  debug!("AWS_LAMBDA_NACOS_ADAPTER_PORT={}", port);
  let cache_size = env::var("AWS_LAMBDA_NACOS_ADAPTER_CACHE_SIZE")
    .ok()
    .and_then(|s| s.parse().ok())
    .unwrap_or(64);
  debug!("AWS_LAMBDA_NACOS_ADAPTER_CACHE_SIZE={}", cache_size);

  let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), port)).await?;
  let (refresh_tx, refresh_rx) = mpsc::channel(1);

  if let Ok(origin) = env::var("AWS_LAMBDA_NACOS_ADAPTER_ORIGIN_ADDRESS") {
    debug!("AWS_LAMBDA_NACOS_ADAPTER_ORIGIN_ADDRESS={}", origin);
    tokio::spawn(start_nacos_adapter(
      listener,
      refresh_rx,
      ProxyConfigProvider::new(cache_size, origin),
    ));
  } else {
    let prefix =
      env::var("AWS_LAMBDA_NACOS_ADAPTER_CONFIG_PATH").unwrap_or("/mnt/efs/nacos/".to_string());
    debug!("AWS_LAMBDA_NACOS_ADAPTER_CONFIG_PATH={}", prefix);

    tokio::spawn(start_nacos_adapter(
      listener,
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
          trace!("got next invocation");
          refresh_tx.send(()).await?;
          // TODO: wait for refresh done?
        }
      }
      Ok(()) as Result<(), Error>
    }
  }))
  .await
}
