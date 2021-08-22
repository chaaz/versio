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
    FromUtf(std::string::FromUtf8Error);
    Glob(glob::PatternError);
    Xml(xmlparser::Error);
    Log(log::SetLoggerError);
    Octo(octocrab::Error);
    Liquid(liquid::Error);
    Ignore(ignore::Error);
    Hyper(hyper::Error);
    HyperInvalid(hyper::http::uri::InvalidUri);
  }
}

impl<'a, T: ?Sized> From<std::sync::PoisonError<std::sync::MutexGuard<'a, T>>> for Error {
  fn from(err: std::sync::PoisonError<std::sync::MutexGuard<'a, T>>) -> Error {
    format!("serde yaml error {:?}", err).into()
  }
}

impl From<gpgme::Error> for Error {
  fn from(err: gpgme::Error) -> Error { format!("gpgme error {:?}", err).into() }
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
      Err(e) => return $crate::either::IterEither2::A(once(Err(e.into())))
    }
  };
}

#[macro_export]
macro_rules! try_iter3 {
  ($arg:expr) => {
    match $arg {
      Ok(x) => x,
      Err(e) => return E3::B(once(Err(e.into())))
    }
  };
}

#[macro_export]
macro_rules! assert_ok {
  ($t:expr, $($er:tt)*) => {
    match ($t).then(|| ()).ok_or_else(|| $crate::bad!($($er)*)) {
      Ok(v) => v,
      Err(e) => return Err($crate::errors::Error::from(e))
    }
  }
}
