//! Utilities to find a mark in a XML file.

use crate::errors::Result;
use crate::mark::Mark;
#[cfg(test)]
use crate::scan::parts::ToPart;
use crate::scan::parts::{is_match_str, IntoPartVec, Part};
use crate::scan::Scanner;
use error_chain::bail;
use xmlparser::{ElementEnd, Token, Tokenizer};

pub struct XmlScanner {
  target: Vec<Part>
}

impl XmlScanner {
  pub fn new(target: &str) -> XmlScanner { XmlScanner { target: target.into_part_vec() } }

  #[cfg(test)]
  pub fn from_parts(target: &[&dyn ToPart]) -> XmlScanner { XmlScanner { target: target.into_part_vec() } }
}

impl Scanner for XmlScanner {
  fn build(parts: Vec<Part>) -> XmlScanner { XmlScanner { target: parts } }
  fn find(&self, data: &str) -> Result<Mark> { scan_xml(data, self.target.clone()) }
}

fn scan_xml<P: IntoPartVec>(data: &str, loc: P) -> Result<Mark> {
  let mut parts = loc.into_part_vec();
  parts.reverse();

  if parts.is_empty() {
    bail!("No parts found for XML spec");
  }

  let mut extra_depth = 0;
  let mut on_target = false;

  for token in Tokenizer::from(data) {
    match token? {
      Token::ElementStart { local, .. } => {
        if extra_depth == 0 && is_match_str(local.as_str(), parts.last()) {
          parts.pop();
          if parts.is_empty() {
            on_target = true;
          }
        } else {
          extra_depth += 1;
        }
      }
      Token::ElementEnd { end, .. } if is_ending(&end) => {
        if extra_depth > 0 {
          extra_depth -= 1;
        } else {
          bail!("Couldn't find version in XML: still expecting {:?}", parts);
        }
      }
      Token::Text { text } => {
        if on_target {
          return Ok(Mark::new(text.as_str().into(), text.start()));
        }
      }
      _ => ()
    }
  }

  bail!("Couldn't find version at end of XML: still expecting {:?}", parts)
}

fn is_ending(end: &ElementEnd) -> bool {
  match end {
    ElementEnd::Close(..) | ElementEnd::Empty => true,
    _ => false
  }
}

#[cfg(test)]
mod test {
  use super::XmlScanner;
  use crate::scan::Scanner;

  #[test]
  fn test_xml() {
    let doc = r#"
<version>1.2.3</version>"#;

    let mark = XmlScanner::new("version").find(doc).unwrap();
    assert_eq!("1.2.3", mark.value());
    assert_eq!(10, mark.start());
  }

  #[test]
  fn test_xml_complex() {
    let doc = r#"
<version>
  <thing>
    <version>1.2.3</version>
  </thing>
</version>"#;

    let mark = XmlScanner::new("version.thing.version").find(doc).unwrap();
    assert_eq!("1.2.3", mark.value());
    assert_eq!(34, mark.start());
  }

  #[test]
  fn test_xml_clever() {
    let doc = r#"
<_0>
  <the.version>1.2.3</version>
</_0>"#;

    let mark = XmlScanner::from_parts(&[&"_0", &"the.version"]).find(doc).unwrap();
    assert_eq!("1.2.3", mark.value());
    assert_eq!(21, mark.start());
  }

  #[test]
  fn test_xml_utf8() {
    let doc = r#"
<naïve><versíøn>1.2.3</naïve></versíøn>"#;

    let mark = XmlScanner::new("naïve.versíøn").find(doc).unwrap();
    assert_eq!("1.2.3", mark.value());
    assert_eq!(20, mark.start());
  }
}
