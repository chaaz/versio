//! Utilities to find a mark in a YAML file.

use crate::error::Result;
use crate::parts::{IntoPartVec, Part};
#[cfg(test)]
use crate::parts::ToPart;
use crate::{convert_mark, CharMark, MarkedData, NamedData, Scanner};
use yaml_rust::parser::{Event, MarkedEventReceiver, Parser};
use yaml_rust::scanner::{Marker, TScalarStyle};

pub struct YamlScanner {
  target: Vec<Part>
}

impl YamlScanner {
  pub fn new<P: IntoPartVec>(target: P) -> YamlScanner { YamlScanner { target: target.into_part_vec() } }

  #[cfg(test)]
  pub fn from_parts(target: &[&dyn ToPart]) -> YamlScanner { YamlScanner { target: target.into_part_vec() } }
}

impl Scanner for YamlScanner {
  fn scan(&self, data: NamedData) -> Result<MarkedData> {
    let char_mark = scan_yaml(&data.data(), self.target.clone())?;
    let byte_mark = convert_mark(data.data(), char_mark)?;
    Ok(data.mark(byte_mark))
  }
}

fn scan_yaml<P: IntoPartVec>(data: &str, loc: P) -> Result<CharMark> {
  let mut rcvr = Receiver::new(loc.into_part_vec());
  let mut parser = Parser::new(data.chars());

  parser.load(&mut rcvr, false)?;

  Ok(rcvr.result.unwrap())
}

struct Receiver {
  parts: Vec<Part>,
  stack: Vec<Loc>,
  depth: usize,
  result: Option<CharMark>
}

impl Receiver {
  pub fn new(parts: Vec<Part>) -> Receiver { Receiver { parts, stack: Vec::new(), depth: 0, result: None } }

  fn found_key(&mut self) { self.depth += 1; }

  fn before_object(&mut self) {
    if self.stack.is_empty() {
      return;
    }

    let i = match self.stack.last().unwrap() {
      Loc::Seq(i) => Some(*i),
      _ => None
    };

    if let Some(i) = i {
      if self.stack.len() == self.depth + 1 && self.parts.get(self.depth).unwrap().seq_ind() == i {
        self.depth += 1;
      }
    }
  }

  fn after_object(&mut self, unnest: bool) {
    if unnest && self.stack.len() == self.depth {
      panic!("Completed seq/map without finding value.");
    }

    if self.stack.is_empty() {
      return;
    }

    match self.stack.last_mut().unwrap() {
      Loc::Map(val) => {
        *val = !*val;
      }
      Loc::Seq(i) => {
        *i += 1;
      }
    }
  }

  fn expecting(&self) -> Expect {
    if self.stack.len() < self.depth {
      panic!("Not as much stack as expected.")
    } else if self.stack.len() == self.depth {
      if self.parts.len() <= self.depth {
        Expect::Scalar
      } else {
        match self.parts.get(self.depth).unwrap() {
          Part::Map(_) => Expect::Map,
          Part::Seq(_) => Expect::Seq
        }
      }
    } else if self.stack.len() == self.depth + 1 {
      match self.parts.get(self.depth).unwrap() {
        Part::Map(key) => {
          if !self.stack.last().unwrap().is_map_value() {
            Expect::Key(key.clone())
          } else {
            Expect::None
          }
        }
        _ => Expect::None
      }
    } else {
      Expect::None
    }
  }
}

impl MarkedEventReceiver for Receiver {
  fn on_event(&mut self, ev: Event, mark: Marker) {
    if self.result.is_some() {
      return;
    }

    match ev {
      Event::MappingStart(_) => {
        self.before_object();
        match self.expecting() {
          Expect::Map => (),
          Expect::None => (),
          _ => panic!("Got unexpected map.")
        }
        self.stack.push(Loc::Map(false));
      }
      Event::MappingEnd => {
        self.stack.pop();
        self.after_object(true);
      }
      Event::SequenceStart(_) => {
        self.before_object();
        match self.expecting() {
          Expect::Seq => (),
          Expect::None => (),
          _ => panic!("Got unexpected seq.")
        }
        self.stack.push(Loc::Seq(0));
      }
      Event::SequenceEnd => {
        self.stack.pop();
        self.after_object(true);
      }
      Event::Scalar(val, style, _anchor, _tag) => {
        self.before_object();
        match self.expecting() {
          Expect::Scalar => {
            let mut index = mark.index();
            match style {
              TScalarStyle::SingleQuoted | TScalarStyle::DoubleQuoted => {
                index += 1;
              }
              _ => ()
            }
            self.result = Some(CharMark::new(val, index));
          }
          Expect::Key(key) => {
            if val == key {
              self.found_key();
            }
          }
          Expect::None => (),
          _ => panic!("Got unexpected scalar \"{}\". {}, {:?}", val, self.depth, self.stack)
        }
        self.after_object(false);
      }
      _ => ()
    }
  }
}

#[derive(Debug)]
enum Loc {
  Map(bool),
  Seq(usize)
}

impl Loc {
  fn is_map_value(&self) -> bool {
    match self {
      Loc::Map(v) => *v,
      _ => panic!("Loc is not map")
    }
  }
}

#[derive(Debug)]
enum Expect {
  Map,
  Seq,
  Scalar,
  Key(String),
  None
}

#[cfg(test)]
mod test {
  use super::{scan_yaml, YamlScanner};
  use crate::{NamedData, Scanner};

  #[test]
  fn test_yaml() {
    let doc = r#"version: 1.2.3"#;

    let char_mark = scan_yaml(doc, "version").unwrap();
    assert_eq!("1.2.3", char_mark.value());
    assert_eq!(9, char_mark.char_start());
  }

  #[test]
  fn test_long_yaml() {
    let doc = r#"
name: "Bob"
thing:
  - first
  - second: 1
  - third: |
      yo
      ho
  - fourth: >
      hey
      yo
    other_x: '2.4.6'
  - hmmm
  - version: 1.2.3
  - this is long"#;

    let char_mark = scan_yaml(doc, "thing.3.other_x").unwrap();
    assert_eq!("2.4.6", char_mark.value());
    assert_eq!(122, char_mark.char_start());
  }

  #[test]
  fn test_yaml_load_utf8() {
    let doc = r#"
name: "Bób"
thing:
  - version: 1.2.3"#;

    let marked_data = YamlScanner::new("thing.0.version").scan(NamedData::new(None, doc.to_string())).unwrap();
    assert_eq!("1.2.3", marked_data.value());
    assert_eq!(34, marked_data.start());
  }

  #[test]
  fn test_yaml_basic() {
    let doc = r#"
package:
  - version: "0.0.6""#;

    let marked_data = YamlScanner::new("package.0.version").scan(NamedData::new(None, doc.to_string())).unwrap();
    assert_eq!("0.0.6", marked_data.value());
    assert_eq!(24, marked_data.start());
  }

  #[test]
  fn test_yaml_clever() {
    let doc = r#"
package:
  0: { the.version: "0.0.6" }"#;

    // "package.0.the.version" doesn't work here.
    let marked_data =
      YamlScanner::from_parts(&[&"package", &"0", &"the.version"]).scan(NamedData::new(None, doc.to_string())).unwrap();
    assert_eq!("0.0.6", marked_data.value());
    assert_eq!(31, marked_data.start());
  }
}
