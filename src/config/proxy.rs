use super::{provider::ConfigProvider, Config};
use lambda_extension::Error;
use moka::future::Cache;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct ProxyConfigProvider {
  cache: Cache<String, Arc<Config>>,
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
  ) -> Result<Arc<Config>, Error> {
    let content = reqwest::get({
      if let Some(tenant) = tenant {
        reqwest::Url::parse_with_params(
          &self.base,
          [("dataId", data_id), ("group", group), ("tenant", tenant)],
        )
      } else {
        reqwest::Url::parse_with_params(&self.base, [("dataId", data_id), ("group", group)])
      }
      .unwrap()
    })
    .await?
    .text()
    .await?;
    let config = Config::new(content);

    // check if cache hit
    let path = format!("{}/{}/{}", tenant.unwrap_or("public"), group, data_id);
    let cache = self.cache.get(&path).await;
    if let Some(cache) = cache {
      if cache.md5() == config.md5() {
        return Ok(cache);
      }
    }

    // cache miss
    let config = Arc::new(config);
    self.cache.insert(path, config.clone()).await;
    Ok(config)
  }
}
