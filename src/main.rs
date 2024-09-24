mod config;
mod constant;
mod grpc;
mod http;

use crate::config::{fs::FsConfigProvider, proxy::ProxyConfigProvider};
use config::{provider::ConfigProvider, target::spawn_target_manager};
use lambda_extension::{
  service_fn,
  tracing::{self, debug, trace},
  Error, LambdaEvent, NextEvent,
};
use std::{
  env,
  fmt::Display,
  net::{Ipv4Addr, SocketAddrV4},
  str::FromStr,
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

  let port = parse_env("AWS_LAMBDA_NACOS_ADAPTER_PORT", 8848);
  let cache_size = parse_env("AWS_LAMBDA_NACOS_ADAPTER_CACHE_SIZE", 64);
  let delay_ms = parse_env("AWS_LAMBDA_NACOS_ADAPTER_DELAY_MS", 10);
  let cooldown_ms = parse_env("AWS_LAMBDA_NACOS_ADAPTER_COOLDOWN_MS", 0);

  let refresh_tx = if let Ok(origin) = env::var("AWS_LAMBDA_NACOS_ADAPTER_ORIGIN_ADDRESS") {
    debug!("AWS_LAMBDA_NACOS_ADAPTER_ORIGIN_ADDRESS={}", origin);
    let cp = ProxyConfigProvider::new(origin);
    start_mock_nacos(port, cp).await?
  } else {
    let prefix =
      env::var("AWS_LAMBDA_NACOS_ADAPTER_CONFIG_PATH").unwrap_or("/mnt/efs/nacos/".to_string());
    debug!("AWS_LAMBDA_NACOS_ADAPTER_CONFIG_PATH={}", prefix);

    let cp = FsConfigProvider::new(cache_size, prefix);
    start_mock_nacos(port, cp).await?
  };

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
            // we don't need to refresh config, just return
            return Ok(());
          }

          trace!("cooldown reached");
          last_refresh_tx
            .send(Instant::now())
            .expect("last_refresh_rx should not be dropped");

          let (changed_tx, mut changed_rx) = mpsc::channel::<()>(1);
          refresh_tx.send(changed_tx).await?;
          let mut changed = false;
          while let Some(()) = changed_rx.recv().await {
            changed = true;
          }
          // now changed_rx.recv() is None, meaning all changed_tx are dropped and the refresh is done
          trace!(changed, "refresh done");

          // only delay if config changed
          if changed && delay_ms > 0 {
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

async fn start_mock_nacos(
  port: u16,
  cp: impl ConfigProvider + Clone + Send + 'static,
) -> Result<mpsc::Sender<mpsc::Sender<()>>, Error> {
  let (refresh_tx, refresh_rx) = mpsc::channel(1);
  let (target_tx, config_tx) = spawn_target_manager(cp.clone(), refresh_rx);

  http::spawn(
    TcpListener::bind(local_addr(port)).await?,
    target_tx.clone(),
    config_tx.clone(),
    cp.clone(),
  );
  grpc::spawn(local_addr(port + 1000).into(), target_tx, config_tx, cp);

  Ok(refresh_tx)
}

fn local_addr(port: u16) -> SocketAddrV4 {
  SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), port)
}

fn parse_env<T: FromStr + Display + Copy>(name: &str, default: T) -> T {
  let v = env::var(name)
    .ok()
    .and_then(|p| p.parse().ok())
    .unwrap_or(default);
  debug!("{}={}", name, default);
  v
}
