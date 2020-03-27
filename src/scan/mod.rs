mod json;
pub mod parts;
mod toml;
mod yaml;

pub use self::toml::TomlScanner;
use crate::{error::Result, MarkedData, NamedData};
pub use json::JsonScanner;
pub use yaml::YamlScanner;

pub trait Scanner {
  fn scan(&self, data: NamedData) -> Result<MarkedData>;
}
