//! Versio is a version management utility.

#![recursion_limit = "1024"]

#[macro_use]
extern crate error_chain;

#[macro_use]
pub mod errors {
  error_chain! {
    links {
      Gql(github_gql::errors::Error, github_gql::errors::ErrorKind);
    }

    foreign_links {
      Num(std::num::ParseIntError);
      Io(std::io::Error);
      Git2(git2::Error);
      Yaml(yaml_rust::scanner::ScanError);
      SerdeYaml(serde_yaml::Error);
      SerdeJson(serde_json::Error);
      Toml(toml::de::Error);
      Regex(regex::Error);
      Utf(std::str::Utf8Error);
      Glob(glob::PatternError);
      Xml(xmlparser::Error);
      Log(log::SetLoggerError);
    }
  }

  impl<'a, T: ?Sized> From<std::sync::PoisonError<std::sync::MutexGuard<'a, T>>> for Error {
    fn from(err: std::sync::PoisonError<std::sync::MutexGuard<'a, T>>) -> Error {
      format!("serde yaml error {:?}", err).into()
    }
  }

  #[macro_export]
  macro_rules! err {
    ($($arg:tt)*) => (std::result::Result::Err(crate::errors::Error::from_kind(crate::errors::ErrorKind::Msg(format!($($arg)*)))))
  }

  #[macro_export]
  macro_rules! bad {
    ($($arg:tt)*) => (crate::errors::Error::from_kind(crate::errors::ErrorKind::Msg(format!($($arg)*))))
  }
}

pub mod analyze;
pub mod config;
pub mod either;
pub mod git;
pub mod github;
pub mod mark;
pub mod mono;
pub mod opts;
pub mod output;
pub mod scan;
pub mod state;
pub mod vcs;

// #[macro_use]
// extern crate error_chain;

