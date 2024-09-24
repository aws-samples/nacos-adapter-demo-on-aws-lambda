use super::Config;
use lambda_extension::Error;
use std::{future::Future, sync::Arc};

/// This should be cheap to clone.
pub trait ConfigProvider: Clone + Send + Sync {
  fn get(
    &mut self,
    data_id: &str,
    group: &str,
    tenant: Option<&str>,
    refresh: bool,
  ) -> impl Future<Output = Result<Arc<Config>, Error>> + Send;
}
