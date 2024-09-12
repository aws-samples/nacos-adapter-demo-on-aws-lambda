pub mod fs;
pub mod provider;
pub mod proxy;

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
