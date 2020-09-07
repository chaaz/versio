//! Error handling for Versio is all based on `error-chain`.

use error_chain::error_chain;

error_chain! {
  links {
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
    Octo(octocrab::Error);
  }
}

impl<'a, T: ?Sized> From<std::sync::PoisonError<std::sync::MutexGuard<'a, T>>> for Error {
  fn from(err: std::sync::PoisonError<std::sync::MutexGuard<'a, T>>) -> Error {
    format!("serde yaml error {:?}", err).into()
  }
}

#[macro_export]
macro_rules! err {
  ($($arg:tt)*) => (
    std::result::Result::Err($crate::errors::Error::from_kind($crate::errors::ErrorKind::Msg(format!($($arg)*))))
  )
}

#[macro_export]
macro_rules! bad {
  ($($arg:tt)*) => ($crate::errors::Error::from_kind($crate::errors::ErrorKind::Msg(format!($($arg)*))))
}

#[macro_export]
macro_rules! try_iter {
  ($arg:expr) => {
    match $arg {
      Ok(x) => x,
      Err(e) => return E2::A(once(Err(e.into())))
    }
  };
}

//   if dir.join("package.json").exists() {
//     let data = match std::fs::read_to_string(&dir.join("package.json")) {
//       Ok(data) => data,
//       Err(e) => return E2::A(once(Err(e.into())))
//     };
//     let name = match JsonScanner::new("name").find(&data) {
//       Ok(name) => name.value().to_string(),
//       Err(e) => return E2::A(once(Err(e)))
//     };
//     summs.push(Ok(ProjSummary::new_file(name, dir.to_string_lossy(), "package.json", "json", "version")));
//   }
