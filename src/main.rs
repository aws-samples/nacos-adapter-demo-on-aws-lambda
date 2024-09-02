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
  time::Duration,
};
use tokio::{
  net::TcpListener,
  sync::{mpsc, watch},
  time::{sleep, Instant},
};

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
  let delay_ms = env::var("AWS_LAMBDA_NACOS_ADAPTER_DELAY_MS")
    .ok()
    .and_then(|d| d.parse().ok())
    .unwrap_or(10);
  debug!("AWS_LAMBDA_NACOS_ADAPTER_DELAY_MS={}", delay_ms);
  let cooldown_ms = env::var("AWS_LAMBDA_NACOS_ADAPTER_COOLDOWN_MS")
    .ok()
    .and_then(|c| c.parse().ok())
    .unwrap_or(5000);

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

  let (last_refresh_tx, last_refresh_rx) = watch::channel(Instant::now());
  lambda_extension::run(service_fn(move |event: LambdaEvent| {
    let refresh_tx = refresh_tx.clone();
    let last_refresh_tx = last_refresh_tx.clone();
    let last_refresh_rx = last_refresh_rx.clone();
    async move {
      match event.next {
        NextEvent::Shutdown(_e) => {}
        NextEvent::Invoke(_e) => {
          trace!("got next invocation");

          if last_refresh_rx.borrow().elapsed().as_millis() < cooldown_ms {
            trace!("cooldown not reached");
            return Ok(());
          }
          trace!("cooldown reached");
          last_refresh_tx
            .send(Instant::now())
            .expect("last_refresh_rx should not be dropped");

          let (done_tx, mut done_rx) = mpsc::channel::<()>(1);
          refresh_tx.send(done_tx).await?;
          // we don't use done_tx to send message,
          // we just wait for all done_tx are dropped
          done_rx.recv().await;
          trace!("refresh done");
          if delay_ms > 0 {
            trace!("sleeping for {}ms", delay_ms);
            sleep(Duration::from_millis(delay_ms)).await;
          }
        }
      }
      Ok(()) as Result<(), Error>
    }
  }))
  .await
}
