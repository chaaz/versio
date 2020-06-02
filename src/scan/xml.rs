//! Utilities to find a mark in a XML file.

use crate::error::Result;
#[cfg(test)]
use crate::scan::parts::ToPart;
use crate::scan::parts::{IntoPartVec, Part, is_match_str};
use crate::scan::Scanner;
use crate::source::{Mark, MarkedData, NamedData};
use xmlparser::{Tokenizer, Token, ElementEnd};

pub struct XmlScanner {
  target: Vec<Part>
}

impl XmlScanner {
  pub fn new<P: IntoPartVec>(target: P) -> XmlScanner { XmlScanner { target: target.into_part_vec() } }

  #[cfg(test)]
  pub fn from_parts(target: &[&dyn ToPart]) -> XmlScanner { XmlScanner { target: target.into_part_vec() } }
}

impl Scanner for XmlScanner {
  fn scan(&self, data: NamedData) -> Result<MarkedData> {
    let byte_mark = scan_xml(data.data(), self.target.clone())?;
    Ok(data.mark(byte_mark))
  }
}

fn scan_xml<P: IntoPartVec>(data: &str, loc: P) -> Result<Mark> {
  let mut parts = loc.into_part_vec();
  parts.reverse();

  if parts.is_empty() {
    return versio_err!("No parts found for XML spec");
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
          return versio_err!("Couldn't find version in XML: still expecting {:?}", parts);
        }
      }
      Token::Text { text } => {
        if on_target {
          return Mark::make(text.as_str().into(), text.start())
        }
      }
      _ => ()
    }
  }

  versio_err!("Couldn't find version at end of XML: still expecting {:?}", parts)
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
  use crate::{scan::Scanner, source::NamedData};

  #[test]
  fn test_xml() {
    let doc = r#"
version = "1.2.3""#;

    let marked_data = XmlScanner::new("version").scan(NamedData::new(None, doc.to_string())).unwrap();
    assert_eq!("1.2.3", marked_data.value());
    assert_eq!(12, marked_data.start());
  }

  #[test]
  fn test_xml_seq() {
    let doc = r#"
thing = [ "thing2", "1.2.3" ]"#;

    let marked_data = XmlScanner::new("thing.1").scan(NamedData::new(None, doc.to_string())).unwrap();
    assert_eq!("1.2.3", marked_data.value());
    assert_eq!(22, marked_data.start());
  }

  #[test]
  fn test_xml_complex() {
    let doc = r#"
[version]
"thing" = [ "2.4.6", { "version" = "1.2.3" } ]"#;

    let marked_data = XmlScanner::new("version.thing.1.version").scan(NamedData::new(None, doc.to_string())).unwrap();
    assert_eq!("1.2.3", marked_data.value());
    assert_eq!(47, marked_data.start());
  }

  #[test]
  fn test_xml_clever() {
    let doc = r#"
[[0]]
"the.version" = "1.2.3""#;

    let marked_data =
      XmlScanner::from_parts(&[&"0", &0, &"the.version"]).scan(NamedData::new(None, doc.to_string())).unwrap();
    assert_eq!("1.2.3", marked_data.value());
    assert_eq!(24, marked_data.start());
  }

  #[test]
  fn test_xml_utf8() {
    let doc = r#"
"thíng" = [ "thíng2", "1.2.3" ]"#;

    let marked_data = XmlScanner::new("thíng.1").scan(NamedData::new(None, doc.to_string())).unwrap();
    assert_eq!("1.2.3", marked_data.value());
    assert_eq!(26, marked_data.start());
  }
}
