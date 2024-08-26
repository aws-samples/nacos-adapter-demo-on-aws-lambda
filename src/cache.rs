use moka::future::Cache as Inner;
use std::{io, os::unix::fs::MetadataExt, sync::Arc};
use tokio::fs;

#[derive(Clone, Debug)]
struct CacheValue {
  mtime: i64,
  content: Arc<String>,
}

#[derive(Clone, Debug)]
pub struct Cache {
  data: Inner<String, CacheValue>,
}

impl Cache {
  pub fn new(size: u64) -> Self {
    Cache {
      data: Inner::new(size),
    }
  }

  pub async fn get(&mut self, path: String) -> Result<Arc<String>, io::Error> {
    let time = fs::metadata(&path).await?.mtime();

    if let Some(value) = self.data.get(&path).await {
      if value.mtime == time {
        // cache hit
        return Ok(value.content);
      }
    }

    // cache miss
    let content = Arc::new(fs::read_to_string(&path).await?);
    self
      .data
      .insert(
        path,
        CacheValue {
          mtime: time,
          content: content.clone(),
        },
      )
      .await;
    Ok(content)
  }
}
