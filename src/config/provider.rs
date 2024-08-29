use lambda_extension::Error;
use std::{future::Future, sync::Arc};

#[derive(Clone, Debug)]
pub struct Config {
  content: String,
  md5: String,
}

impl Config {
  pub fn new(content: String) -> Self {
    Config {
      md5: format!("{:x}", md5::compute(&content)),
      content,
    }
  }

  pub fn content(&self) -> &str {
    &self.content
  }

  pub fn md5(&self) -> &str {
    &self.md5
  }
}

pub trait ConfigProvider {
  fn get(
    &mut self,
    data_id: &str,
    group: &str,
    tenant: Option<&str>,
  ) -> impl Future<Output = Result<Arc<Config>, Error>> + Send;
}
