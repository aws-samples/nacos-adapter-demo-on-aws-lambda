use super::{provider::ConfigProvider, Config};
use lambda_extension::Error;
use moka::future::Cache;
use std::{os::unix::fs::MetadataExt, sync::Arc};
use tokio::fs;

/// This is cheap to clone.
#[derive(Clone, Debug)]
pub struct CacheValue {
  pub mtime: i64,
  pub config: Arc<Config>,
}

#[derive(Clone, Debug)]
pub struct FsConfigProvider {
  cache: Cache<String, CacheValue>,
  prefix: String,
}

impl FsConfigProvider {
  pub fn new(size: u64, prefix: String) -> Self {
    FsConfigProvider {
      cache: Cache::new(size),
      prefix,
    }
  }
}

impl ConfigProvider for FsConfigProvider {
  async fn get(
    &mut self,
    data_id: &str,
    group: &str,
    tenant: Option<&str>,
  ) -> Result<Arc<Config>, Error> {
    let path = format!(
      "{}{}/{}/{}",
      self.prefix,
      tenant.unwrap_or("public"),
      group,
      data_id
    );

    // check cache by mtime
    let mtime = fs::metadata(&path).await?.mtime();
    if let Some(value) = self.cache.get(&path).await {
      if value.mtime == mtime {
        // cache hit
        return Ok(value.config);
      }
    }

    // cache miss
    let content = fs::read_to_string(&path).await?;
    let config = Arc::new(Config::new(content));
    self
      .cache
      .insert(
        path,
        CacheValue {
          mtime,
          config: config.clone(),
        },
      )
      .await;
    Ok(config)
  }
}
