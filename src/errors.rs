//! Error handling for Versio is all based on `error-chain`.

pub use anyhow::{Context, Error, Result};

// impl<'a, T: ?Sized> From<std::sync::PoisonError<std::sync::MutexGuard<'a, T>>> for Error {
//   fn from(err: std::sync::PoisonError<std::sync::MutexGuard<'a, T>>) -> Error {
//     format!("serde yaml error {:?}", err).into()
//   }
// }
//
// impl From<gpgme::Error> for Error {
//   fn from(err: gpgme::Error) -> Error { format!("gpgme error {:?}", err).into() }
// }

#[macro_export]
macro_rules! err {
  ($($arg:tt)*) => (std::result::Result::Err(anyhow::anyhow!($($arg)*)))
}

#[macro_export]
macro_rules! bad {
  ($($arg:tt)*) => (anyhow::anyhow!($($arg)*))
}

#[macro_export]
macro_rules! bail {
  ($($arg:tt)*) => (anyhow::bail!($($arg)*))
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
      Err(e) => return $crate::err!(e)
    }
  }
}
