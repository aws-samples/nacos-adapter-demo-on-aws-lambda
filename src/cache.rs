use moka::future::Cache as Inner;
use std::{io, os::unix::fs::MetadataExt, sync::Arc};
use tokio::fs;

#[derive(Clone, Debug)]
pub struct CacheValue {
  pub mtime: i64,
  pub content: String,
  pub md5: String,
}

#[derive(Clone, Debug)]
pub struct Cache {
  data: Inner<String, Arc<CacheValue>>,
}

impl Cache {
  pub fn new(size: u64) -> Self {
    Cache {
      data: Inner::new(size),
    }
  }

  pub async fn get(&mut self, path: String) -> Result<Arc<CacheValue>, io::Error> {
    let mtime = fs::metadata(&path).await?.mtime();

    if let Some(value) = self.data.get(&path).await {
      if value.mtime == mtime {
        // cache hit
        return Ok(value);
      }
    }

    // cache miss
    let content = fs::read_to_string(&path).await?;
    let md5 = String::from_utf8_lossy(&md5::compute(&content).0).into_owned();
    let value = Arc::new(CacheValue {
      mtime,
      content,
      md5,
    });
    self.data.insert(path, value.clone()).await;
    Ok(value)
  }
}
