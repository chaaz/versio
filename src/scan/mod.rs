mod json;
mod toml;
mod yaml;

pub use json::JsonScanner;
pub use self::toml::TomlScanner;
pub use yaml::YamlScanner;
use crate::{NamedData, MarkedData, error::Result};

pub trait Scanner {
  fn scan(&self, data: NamedData) -> Result<MarkedData>;
}
