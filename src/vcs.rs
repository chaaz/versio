//! Simple management of the current VCS level that we're running at.

use crate::errors::Result;
use crate::git::Repo;
use error_chain::bail;
use log::debug;
use std::cmp::{max, min};
use std::str::FromStr;

#[derive(Debug)]
pub struct VcsRange {
  min: VcsLevel,
  max: VcsLevel
}

impl VcsRange {
  pub fn new(min: VcsLevel, max: VcsLevel) -> VcsRange { VcsRange { min, max } }
  pub fn full() -> VcsRange { VcsRange { min: VcsLevel::None, max: VcsLevel::Smart } }
  pub fn exact(lvl: VcsLevel) -> VcsRange { VcsRange { min: lvl, max: lvl } }

  pub fn min(&self) -> VcsLevel { self.min }
  pub fn max(&self) -> VcsLevel { self.max }
  pub fn is_empty(&self) -> bool { self.min > self.max }

  pub fn intersect(&self, other: &VcsRange) -> VcsRange {
    VcsRange::new(max(self.min(), other.min()), min(self.max(), other.max()))
  }

  pub fn detect() -> Result<VcsRange> { Ok(VcsRange::new(VcsLevel::None, Repo::detect(".")?)) }

  pub fn detect_and_combine(pref: &VcsRange, reqd: &VcsRange) -> Result<VcsRange> {
    if pref.is_empty() {
      bail!("Preferred VCS {:?} is empty.", pref);
    } else if reqd.is_empty() {
      bail!("Required VCS {:?} is empty.", reqd);
    }

    let i1 = pref.intersect(reqd);
    if i1.is_empty() {
      if pref.min() > reqd.max() {
        bail!("Preferred VCS {:?} grtr than required {:?}.", pref, reqd);
      } else {
        bail!("Preferred VCS {:?} less than required {:?}.", pref, reqd);
      }
    }

    let dctd = VcsRange::detect()?;
    let i2 = i1.intersect(&dctd);
    if i2.is_empty() {
      bail!("Couldn't detect {:?} with preferred {:?} required {:?}", dctd, pref, reqd);
    }

    debug!("Combining preferred {:?}, required {:?}, detected {:?} = {:?}", pref, reqd, dctd, i2.max());

    Ok(i2)
  }
}

#[derive(PartialEq, PartialOrd, Eq, Ord, Clone, Copy, Debug)]
pub enum VcsLevel {
  None = 0,
  Local = 1,
  Remote = 2,
  Smart = 3
}

impl FromStr for VcsLevel {
  type Err = crate::errors::Error;

  fn from_str(v: &str) -> Result<VcsLevel> {
    match v {
      "none" => Ok(VcsLevel::None),
      "local" => Ok(VcsLevel::Local),
      "remote" => Ok(VcsLevel::Remote),
      "smart" => Ok(VcsLevel::Smart),
      other => err!("Illegal vcs level \"{}\".", other)
    }
  }
}
