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
use regex::Regex;

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

pub fn find_reg_data(data: &str, pattern: &str) -> Result<Mark> {
  let pattern = Regex::new(pattern)?;
  let found = pattern.captures(data).ok_or_else(|| bad!("No match for {}", pattern))?;
  let item = found.get(1).ok_or_else(|| bad!("No capture group in {}.", pattern))?;
  let value = item.as_str().to_string();
  let index = item.start();
  Ok(Mark::new(value, index))
}

pub fn scan_reg_data(data: NamedData, pattern: &str) -> Result<MarkedData> {
  let mark = find_reg_data(data.data(), pattern)?;
  Ok(data.mark(mark))
}
