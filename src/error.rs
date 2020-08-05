//! Some core error/result structures.

#[macro_export]
macro_rules! versio_err {
  ($($arg:tt)*) => (Err(crate::error::Error::new(format!($($arg)*))))
}

#[macro_export]
macro_rules! versio_error {
  ($($arg:tt)*) => (crate::error::Error::new(format!($($arg)*)))
}

#[derive(Debug)]
pub struct Error {
  description: String
}

impl Error {
  pub fn new<S: ToString>(s: S) -> Error { Error { description: s.to_string() } }
}

impl From<std::num::ParseIntError> for Error {
  fn from(err: std::num::ParseIntError) -> Error { Error { description: err.to_string() } }
}

impl From<std::io::Error> for Error {
  fn from(err: std::io::Error) -> Error { Error { description: format!("io error {:?}", err) } }
}

impl<'a, T: ?Sized> From<std::sync::PoisonError<std::sync::MutexGuard<'a, T>>> for Error {
  fn from(err: std::sync::PoisonError<std::sync::MutexGuard<'a, T>>) -> Error {
    Error { description: format!("serde yaml error {:?}", err) }
  }
}

impl From<git2::Error> for Error {
  fn from(err: git2::Error) -> Error { Error { description: format!("git error {:?}", err) } }
}

impl From<yaml_rust::scanner::ScanError> for Error {
  fn from(err: yaml_rust::scanner::ScanError) -> Error { Error { description: format!("yaml error {:?}", err) } }
}

impl From<serde_yaml::Error> for Error {
  fn from(err: serde_yaml::Error) -> Error { Error { description: format!("serde yaml error {:?}", err) } }
}

impl From<serde_json::Error> for Error {
  fn from(err: serde_json::Error) -> Error { Error { description: format!("serde yaml error {:?}", err) } }
}

impl From<toml::de::Error> for Error {
  fn from(err: toml::de::Error) -> Error { Error { description: format!("serde toml error {:?}", err) } }
}

impl From<regex::Error> for Error {
  fn from(err: regex::Error) -> Error { Error { description: format!("regex error {:?}", err) } }
}

impl From<std::str::Utf8Error> for Error {
  fn from(err: std::str::Utf8Error) -> Error { Error { description: format!("utf8 error {:?}", err) } }
}

impl From<glob::PatternError> for Error {
  fn from(err: glob::PatternError) -> Error { Error { description: format!("glob error {:?}", err) } }
}

impl From<github_gql::errors::Error> for Error {
  fn from(err: github_gql::errors::Error) -> Error { Error { description: format!("github error {:?}", err) } }
}

impl From<xmlparser::Error> for Error {
  fn from(err: xmlparser::Error) -> Error { Error { description: format!("xmlparser error {:?}", err) } }
}

impl From<log::SetLoggerError> for Error {
  fn from(err: log::SetLoggerError) -> Error { Error { description: format!("log init error {:?}", err) } }
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_construct_err() { let _e: Error = Error { description: "This is a test.".into() }; }

  #[test]
  fn test_debug_err() { let _e: String = format!("Error: {:?}", Error { description: "This is a test.".into() }); }

  #[test]
  fn test_parse_err() { let _e: Error = "not a number".parse::<u32>().unwrap_err().into(); }

  #[test]
  fn test_io_err() {
    use std::io::{Error as IoError, ErrorKind};
    let _e: Error = IoError::new(ErrorKind::Other, "test error").into();
  }
}
