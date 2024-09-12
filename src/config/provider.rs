use super::Config;
use lambda_extension::Error;
use std::{future::Future, sync::Arc};

pub trait ConfigProvider {
  fn get(
    &mut self,
    data_id: &str,
    group: &str,
    tenant: Option<&str>,
  ) -> impl Future<Output = Result<Arc<Config>, Error>> + Send;
}
