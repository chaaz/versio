//! Utilities to find a mark in a YAML file.

use crate::error::Result;
use crate::Mark;
use yaml_rust::parser::{Parser, MarkedEventReceiver, Event};
use yaml_rust::scanner::{Marker, TScalarStyle};
use std::sync::{Arc, Mutex};

pub fn scan_yaml<P: ToPartVec>(file: &str, loc: P) -> Result<Mark> {
  let mut rcvr = Receiver::new(loc.to_part_vec());
  let mut parser = Parser::new(rcvr.shortcut(file.chars()));

  parser.load(&mut rcvr, false)?;

  Ok(rcvr.result.unwrap())
}

pub trait ToPartVec {
  fn to_part_vec(&self) -> Vec<Part>;
}

impl ToPartVec for str {
  fn to_part_vec(&self) -> Vec<Part> {
    self.split('.').map(|d| {
      match d.parse() {
        Ok(i) => Part::Seq(i),
        Err(_) => Part::Map(d.to_string())
      }
    }).collect()
  }
}

impl ToPartVec for &str {
  fn to_part_vec(&self) -> Vec<Part> { (*self).to_part_vec() }
}

impl ToPartVec for [&dyn ToPart] {
  fn to_part_vec(&self) -> Vec<Part> { self.iter().map(|d| d.to_part()).collect() }
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
  fn to_part(&self) -> Part { 
    Part::Seq(*self)
  }
}

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
  result: Option<Mark>
}

impl Receiver {
  pub fn new(parts: Vec<Part>) -> Receiver {
    Receiver { parts, stack: Vec::new(), depth: 0, complete: Arc::new(Mutex::new(false)), result: None }
  }

  pub fn shortcut<T: Iterator<Item = char>>(&self, t: T) -> Shortcut<T> {
    Shortcut::new(self.complete.clone(), t)
  }

  fn found_key(&mut self) {
    self.depth += 1;
  }

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
          Part::Seq(_) => Expect::Seq,
        }
      }
    } else if self.stack.len() == self.depth + 1 {
      match self.parts.get(self.depth).unwrap() {
        Part::Map(key) =>  {
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
          _ => {
            panic!("Got unexpected map.")
          }
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
            let mut index = mark.index() - 1;
            match style {
              TScalarStyle::SingleQuoted | TScalarStyle::DoubleQuoted => { index += 1; }
              _ => ()
            }
            self.result = Some(Mark::new(val.to_string(), index));
            *self.complete.lock().unwrap() = true;
          }
          Expect::Key(key) => {
            if val == key {
              self.found_key();
            }
          }
          Expect::None => (),
          _ => panic!("Got unexpected scalar.")
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
