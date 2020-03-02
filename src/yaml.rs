//! Utilities to find a mark in a YAML file.

use crate::error::Result;
use crate::{convert_mark, CharMark, Load, MarkedData};
use std::fs::read_to_string;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use yaml_rust::parser::{Event, MarkedEventReceiver, Parser};
use yaml_rust::scanner::{Marker, TScalarStyle};

pub struct YamlLoad {
  target: Vec<Part>
}

impl YamlLoad {
  pub fn new<P: IntoPartVec>(target: P) -> YamlLoad { YamlLoad { target: target.into_part_vec() } }
}

impl Load for YamlLoad {
  fn load<P: AsRef<Path>>(&self, filename: P) -> Result<MarkedData> {
    let data = read_to_string(&filename)?;
    self.read(data, Some(filename.as_ref().to_path_buf()))
  }

  fn read(&self, data: String, fname: Option<PathBuf>) -> Result<MarkedData> {
    let char_mark = scan_yaml(&data, self.target.clone())?;
    let byte_mark = convert_mark(&data, char_mark);

    Ok(MarkedData::new(fname, data.to_string(), byte_mark))
  }
}

fn scan_yaml<P: IntoPartVec>(data: &str, loc: P) -> Result<CharMark> {
  let mut rcvr = Receiver::new(loc.into_part_vec());
  let mut parser = Parser::new(rcvr.shortcut(data.chars()));

  parser.load(&mut rcvr, false)?;

  Ok(rcvr.result.unwrap())
}

pub trait IntoPartVec {
  fn into_part_vec(self) -> Vec<Part>;
}

impl IntoPartVec for Vec<Part> {
  fn into_part_vec(self) -> Vec<Part> { self }
}

impl IntoPartVec for &str {
  fn into_part_vec(self) -> Vec<Part> { self.split('.').map(|d| d.to_part()).collect() }
}

impl IntoPartVec for &[&dyn ToPart] {
  fn into_part_vec(self) -> Vec<Part> { self.iter().map(|d| d.to_part()).collect() }
}

pub trait ToPart {
  fn to_part(&self) -> Part;
}

impl ToPart for str {
  fn to_part(&self) -> Part {
    match self.parse() {
      Ok(i) => Part::Seq(i),
      Err(_) => Part::Map(self.to_string())
    }
  }
}

impl ToPart for usize {
  fn to_part(&self) -> Part { Part::Seq(*self) }
}

#[derive(Clone)]
pub enum Part {
  Seq(usize),
  Map(String)
}

impl Part {
  fn seq_ind(&self) -> usize {
    match self {
      Part::Seq(i) => *i,
      _ => panic!("Part is not seq")
    }
  }
}

struct Receiver {
  parts: Vec<Part>,
  stack: Vec<Loc>,
  depth: usize,
  complete: Arc<Mutex<bool>>,
  result: Option<CharMark>
}

impl Receiver {
  pub fn new(parts: Vec<Part>) -> Receiver {
    Receiver { parts, stack: Vec::new(), depth: 0, complete: Arc::new(Mutex::new(false)), result: None }
  }

  pub fn shortcut<T: Iterator<Item = char>>(&self, t: T) -> Shortcut<T> { Shortcut::new(self.complete.clone(), t) }

  fn found_key(&mut self) { self.depth += 1; }

  fn before_object(&mut self) {
    if self.stack.is_empty() {
      return;
    }

    let i = match self.stack.last().unwrap() {
      Loc::Seq(i) => Some(*i),
      _ => None
    };

    match i {
      Some(i) => {
        if self.stack.len() == self.depth + 1 {
          if self.parts.get(self.depth).unwrap().seq_ind() == i {
            self.depth += 1;
          }
        }
      }
      _ => ()
    }
  }

  fn after_object(&mut self, unnest: bool) {
    if unnest && self.stack.len() == self.depth {
      panic!("Unexpected parity.");
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
            self.result = Some(CharMark::new(val.to_string(), index));
            *self.complete.lock().unwrap() = true;
          }
          Expect::Key(key) => {
            if val == key {
              self.found_key();
            }
          }
          Expect::None => (),
          _ => panic!("Got unexpected scalar. {}, {:?}", self.depth, self.stack)
        }
        self.after_object(false);
      }
      _ => ()
    }
  }
}

struct Shortcut<T> {
  complete: Arc<Mutex<bool>>,
  iter: T
}

impl<T> Shortcut<T> {
  pub fn new(complete: Arc<Mutex<bool>>, iter: T) -> Shortcut<T> { Shortcut { complete, iter } }
}

impl<T: Iterator<Item = char>> Iterator for Shortcut<T> {
  type Item = char;

  fn next(&mut self) -> Option<char> {
    match *self.complete.lock().unwrap() {
      true => None,
      false => self.iter.next()
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
  use super::{scan_yaml, Load, YamlLoad};

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

    let marked_data = YamlLoad::new("thing.0.version").read(doc.to_string(), None).unwrap();
    assert_eq!("1.2.3", marked_data.value());
    assert_eq!(34, marked_data.start());
  }
}
