use super::{provider::ConfigProvider, Config};
use lambda_extension::Error;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct ProxyConfigProvider {
  base: String,
}

impl ProxyConfigProvider {
  pub fn new(addr: String) -> Self {
    ProxyConfigProvider {
      base: format!("http://{}/nacos/v1/cs/configs", addr),
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
      // TODO: simplify code?
      if let Some(tenant) = tenant {
        reqwest::Url::parse_with_params(
          &self.base,
          [("dataId", data_id), ("group", group), ("tenant", tenant)],
        )
      } else {
        reqwest::Url::parse_with_params(&self.base, [("dataId", data_id), ("group", group)])
      }?
    })
    .await?
    .text()
    .await?;

    Ok(Arc::new(Config::new(content)))
  }
}
