mod json;
pub mod parts;
mod toml;
mod xml;
mod yaml;

pub use self::toml::TomlScanner;
pub use self::xml::XmlScanner;
use crate::error::Result;
use crate::source::{MarkedData, NamedData};
pub use json::JsonScanner;
pub use yaml::YamlScanner;

pub trait Scanner {
  fn scan(&self, data: NamedData) -> Result<MarkedData>;
}
