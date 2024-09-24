use super::{provider::ConfigProvider, Config};
use lambda_extension::Error;
use moka::future::Cache;
use reqwest::Url;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct ProxyConfigProvider {
  /// Moka cache, which is cheap to clone.
  cache: Cache<String, Arc<Config>>,
  /// Wrapped in an Arc to make [`Self`] cheap to clone.
  base: Arc<String>,
}

impl ProxyConfigProvider {
  pub fn new(size: u64, addr: String) -> Self {
    ProxyConfigProvider {
      cache: Cache::new(size),
      base: Arc::new(format!("http://{}/nacos/v1/cs/configs", addr)),
    }
  }
}

impl ConfigProvider for ProxyConfigProvider {
  async fn get(
    &mut self,
    data_id: &str,
    group: &str,
    tenant: Option<&str>,
    refresh: bool,
  ) -> Result<Arc<Config>, Error> {
    let key = format!("{}/{}/{}", tenant.unwrap_or(""), group, data_id);

    if !refresh {
      if let Some(value) = self.cache.get(&key).await {
        return Ok(value);
      }
    }

    let content = reqwest::get({
      if let Some(tenant) = tenant {
        Url::parse_with_params(
          &self.base,
          [("dataId", data_id), ("group", group), ("tenant", tenant)],
        )
      } else {
        Url::parse_with_params(&self.base, [("dataId", data_id), ("group", group)])
      }?
    })
    .await?
    .text()
    .await?;

    let config = Arc::new(Config::new(content));
    self.cache.insert(key, config.clone()).await;
    Ok(config)
  }
}
