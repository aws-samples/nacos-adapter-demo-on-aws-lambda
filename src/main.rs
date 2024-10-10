mod config;
mod grpc;
mod http;

use crate::config::{fs::FsConfigProvider, passthrough::PassthroughConfigProvider};
use aws_lambda_runtime_proxy::{LambdaRuntimeApiClient, MockLambdaRuntimeApiServer};
use config::{provider::ConfigProvider, target::spawn_target_manager};
use lambda_extension::{
  service_fn,
  tracing::{debug, subscriber::EnvFilter, warn},
  Error, LambdaEvent, NextEvent,
};
use std::{
  env,
  fmt::Display,
  net::{Ipv4Addr, SocketAddrV4},
  str::FromStr,
};
use tokio::{
  net::TcpListener,
  sync::{mpsc, watch},
  time::Instant,
};
use tracing_subscriber::filter::LevelFilter;

#[tokio::main]
async fn main() -> Result<(), Error> {
  tracing_subscriber::fmt()
    .with_env_filter(
      EnvFilter::builder()
        .with_default_directive(LevelFilter::INFO.into())
        .from_env_lossy(),
    )
    .without_time()
    .init();

  let port = parse_env("AWS_LAMBDA_NACOS_ADAPTER_PORT", 8848);
  let cache_size = parse_env("AWS_LAMBDA_NACOS_ADAPTER_CACHE_SIZE", 64);
  let cooldown_ms = parse_env("AWS_LAMBDA_NACOS_ADAPTER_COOLDOWN_MS", 0);
  let sync_port = parse_env("AWS_LAMBDA_NACOS_ADAPTER_SYNC_PORT", 0);
  let sync_cooldown_ms = parse_env("AWS_LAMBDA_NACOS_ADAPTER_SYNC_COOLDOWN_MS", 0);

  if sync_cooldown_ms < cooldown_ms {
    warn!("AWS_LAMBDA_NACOS_ADAPTER_SYNC_COOLDOWN_MS should be no less than AWS_LAMBDA_NACOS_ADAPTER_COOLDOWN_MS");
  }

  // start mock nacos, try passthrough mode first, otherwise use fs mode
  let refresh_tx = if let Ok(origin) = env::var("AWS_LAMBDA_NACOS_ADAPTER_ORIGIN_ADDRESS") {
    debug!("AWS_LAMBDA_NACOS_ADAPTER_ORIGIN_ADDRESS={}", origin);
    start_mock_nacos(port, PassthroughConfigProvider::new(cache_size, origin)).await?
  } else {
    let prefix = env::var("AWS_LAMBDA_NACOS_ADAPTER_CONFIG_PATH")
      .unwrap_or_else(|_| "/mnt/efs/nacos/".to_string());
    debug!("AWS_LAMBDA_NACOS_ADAPTER_CONFIG_PATH={}", prefix);
    start_mock_nacos(port, FsConfigProvider::new(cache_size, prefix)).await?
  };

  let (last_refresh_setter, last_refresh) = watch::channel(Instant::now());

  // start lambda runtime api proxy if the sync mode is enabled
  if sync_port != 0 {
    let refresh_tx = refresh_tx.clone();
    let last_refresh_setter = last_refresh_setter.clone();
    let last_refresh = last_refresh.clone();
    tokio::spawn(async move {
      MockLambdaRuntimeApiServer::bind(sync_port)
        .await
        .expect("failed to start lambda runtime api proxy")
        .serve(move |req| {
          let refresh_tx = refresh_tx.clone();
          let last_refresh_setter = last_refresh_setter.clone();
          let last_refresh = last_refresh.clone();
          async move {
            let mut client = LambdaRuntimeApiClient::new().await?;
            if req.uri().path() != "/2018-06-01/runtime/invocation/next" {
              // not invocation/next, just forward
              return client.forward(req).await;
            }

            // else, the request is invocation/next, forward the request first
            let res = client.forward(req).await?;
            // now we get the response, we should refresh config before returning the response to the handler

            if last_refresh.borrow().elapsed().as_millis() < sync_cooldown_ms {
              debug!("sync cooldown not reached");
            } else {
              debug!("sync cooldown reached");
              last_refresh_setter
                .send(Instant::now())
                .expect("send last_refresh failed");

              refresh(&refresh_tx).await.expect("refresh failed");
            }

            Ok(res)
          }
        })
        .await
    });
  }

  // start lambda extension
  lambda_extension::run(service_fn(move |event: LambdaEvent| {
    let refresh_tx = refresh_tx.clone();
    let last_refresh_setter = last_refresh_setter.clone();
    let last_refresh = last_refresh.clone();

    async move {
      match event.next {
        NextEvent::Shutdown(_e) => {
          // TODO: print nacos logs? user should provide a file path like /tmp/nacos/logs/nacos/config.log
        }
        NextEvent::Invoke(_e) => {
          let last_refresh = last_refresh.borrow();
          if sync_port != 0 && last_refresh.elapsed().as_millis() >= sync_cooldown_ms {
            // runtime proxy is enabled and it will refresh the config, skip here
            return Ok(());
          }

          if last_refresh.elapsed().as_millis() < cooldown_ms {
            debug!("cooldown not reached");
          } else {
            debug!("cooldown reached");
            drop(last_refresh); // prevent deadlock
            last_refresh_setter.send(Instant::now())?;
            refresh(&refresh_tx).await?;
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
  cp: impl ConfigProvider + 'static,
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
  debug!("{}={}", name, v);
  v
}

async fn refresh(refresh_tx: &mpsc::Sender<mpsc::Sender<()>>) -> Result<(), Error> {
  let (changed_tx, mut changed_rx) = mpsc::channel::<()>(1);
  refresh_tx.send(changed_tx).await?;
  let mut changed = false;
  let now = Instant::now();
  while let Some(()) = changed_rx.recv().await {
    changed = true;
  }
  // now changed_rx.recv() returns None, meaning all changed_tx are dropped and the refresh is done
  debug!(changed, "refresh done: {}ms", now.elapsed().as_millis());

  Ok(())
}
