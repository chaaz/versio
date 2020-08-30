mod json;
pub mod parts;
mod toml;
mod xml;
mod yaml;

pub use self::json::JsonScanner;
pub use self::toml::TomlScanner;
pub use self::xml::XmlScanner;
pub use self::yaml::YamlScanner;
use crate::errors::Result;
use crate::mark::{Mark, MarkedData, NamedData};
use crate::scan::parts::Part;

pub trait Scanner {
  fn build(parts: Vec<Part>) -> Self;

  fn find(&self, data: &str) -> Result<Mark>;

  fn find_version(&self, data: &str) -> Result<Mark> {
    let mark = self.find(data)?;
    mark.validate_version()?;
    Ok(mark)
  }

  fn scan(&self, data: NamedData) -> Result<MarkedData> {
    let mark = self.find(data.data())?;
    Ok(data.mark(mark))
  }
}
