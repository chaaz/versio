//! Utilities to find a mark in a JSON file.

use serde::de::{self, DeserializeSeed, Deserializer, IgnoredAny, SeqAccess, MapAccess, Visitor, Unexpected};
use std::sync::{Arc, Mutex};
use crate::{Load, Mark, MarkedData};
use crate::error::Result;
use std::path::{Path, PathBuf};
use std::fs::read_to_string;

pub type TraceRef = Arc<Mutex<Trace>>;

pub struct JsonLoad {
  target: Vec<Part>
}

impl JsonLoad {
  pub fn new<P: IntoPartVec>(target: P) -> JsonLoad { JsonLoad { target: target.into_part_vec() } }
}

impl Load for JsonLoad {
  fn load<P: AsRef<Path>>(&self, filename: P) -> Result<MarkedData> {
    let data = read_to_string(&filename)?;
    self.read(data, Some(filename.as_ref().to_path_buf()))
  }

  fn read(&self, data: String, fname: Option<PathBuf>) -> Result<MarkedData> {
    let byte_mark = scan_json(&data, self.target.clone())?;
    Ok(MarkedData::new(fname, data.to_string(), byte_mark))
  }
}

fn scan_json<P: IntoPartVec>(data: &str, loc: P) -> Result<Mark> {
  let mut parts = loc.into_part_vec();
  parts.reverse();

  let trace = Arc::new(Mutex::new(Trace::new()));
  let reader = MeteredReader::new(data.as_bytes(), trace.clone());

  let value = pop(parts, trace.clone()).deserialize(&mut serde_json::Deserializer::from_reader(reader))?;
  let index = trace.lock()?.find_start()?;

  Ok(Mark::new(value, index))
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

#[derive(Clone, Debug)]
pub enum Part {
  Seq(usize),
  Map(String)
}

fn pop(mut parts: Vec<Part>, trace: TraceRef) -> NthElement {
  let part = parts.pop().unwrap();
  NthElement::new(part, parts, trace)
}

// A seed that can be used to deserialize only the `n`th element of a sequence
// while efficiently discarding elements of any type before or after index `n`.
//
// For example to deserialize only the element at index 3:
//
//    NthElement::new(3).deserialize(deserializer)
pub struct NthElement {
  part: Part,
  remains: Vec<Part>,
  trace: TraceRef,
  // marker: PhantomData<fn() -> T>
}

impl NthElement {
  pub fn new(part: Part, remains: Vec<Part>, trace: TraceRef) -> NthElement { NthElement { part, remains, trace } }
}

impl<'de> Visitor<'de> for NthElement {
  type Value = String;

  fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
    write!(formatter, "a part that is {:?}", self.part)
  }

  fn visit_map<V>(mut self, mut map: V) -> std::result::Result<Self::Value, V::Error>
  where
    V: MapAccess<'de>
  {
    let expected_key: String = match &self.part {
      Part::Map(key) => key.clone(),
      _ => return Err(de::Error::invalid_type(Unexpected::Map, &self))
    };

    let mut got_val: Option<String> = None;

    while let Some(key) = map.next_key::<String>()? {
      if key == expected_key {
        let nth = match self.remains.is_empty() {
          true => {
            self.trace.lock().unwrap().set_active(true);
            let r = map.next_value()?;
            self.trace.lock().unwrap().set_active(false);
            r
          }
          false => {
            let next = pop(std::mem::replace(&mut self.remains, Vec::new()), self.trace.clone());
            map.next_value_seed(next)?
          }
        };

        got_val = Some(nth);
        break;
      } else {
        drop(map.next_value::<IgnoredAny>()?)
      }
    }

    while let Some((IgnoredAny, IgnoredAny)) = map.next_entry()? {
    }

    let ista = got_val.ok_or_else(|| de::Error::missing_field("<missing field>"))?;
    Ok(ista)
  }

  fn visit_seq<V>(mut self, mut seq: V) -> std::result::Result<Self::Value, V::Error>
  where
    V: SeqAccess<'de>
  {
    let n = match &self.part {
      Part::Seq(n) => *n,
      _ => return Err(de::Error::invalid_type(Unexpected::Seq, &self))
    };

    for i in 0 .. n {
      if seq.next_element::<IgnoredAny>()?.is_none() {
        return Err(de::Error::invalid_length(i, &self));
      }
    }

    let nth = match self.remains.is_empty() {
      true => {
        self.trace.lock().unwrap().set_active(true);
        let r = seq.next_element()?.ok_or_else(|| de::Error::invalid_length(n, &self))?;
        self.trace.lock().unwrap().set_active(false);
        r
      }
      false => {
        let next = pop(std::mem::replace(&mut self.remains, Vec::new()), self.trace.clone());
        seq.next_element_seed(next)?.ok_or_else(|| de::Error::invalid_length(n, &self))?
      }
    };

    while let Some(IgnoredAny) = seq.next_element()? {
    }

    Ok(nth)
  }
}

impl<'de> DeserializeSeed<'de> for NthElement {
  type Value = String;

  fn deserialize<D>(self, deserializer: D) -> std::result::Result<Self::Value, D::Error>
  where
    D: Deserializer<'de>
  {
    deserializer.deserialize_any(self)
  }
}

pub struct Trace {
  active: bool,
  leader: usize,
  bytes: Vec<u8>,
}

impl Trace {
  pub fn new() -> Trace { Trace { active: false, leader: 0, bytes: Vec::new() } }

  pub fn set_active(&mut self, active: bool) {
    self.active = active;
  }

  pub fn accept(&mut self, buf: &[u8], amt: usize, leader: usize) {
    if self.active {
      if self.bytes.is_empty() {
        self.leader = leader;
      }
      self.bytes.extend_from_slice(&buf[.. amt]);
    }
  }

  pub fn find_start(&self) -> crate::error::Result<usize> {
    Ok(
      self.bytes.iter().position(|b| *b == b'"').ok_or_else(|| versio_error!("No quote found in value"))?
        + self.leader + 1
    )
  }
}

struct MeteredReader<'a> {
  data: &'a [u8],
  got: usize,
  trace: TraceRef,
}

impl<'a> MeteredReader<'a> {
  pub fn new(data: &'a [u8], trace: TraceRef) -> MeteredReader { MeteredReader { data, got: 0, trace } }
}

impl<'a> std::io::Read for MeteredReader<'a> {
  fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
    let amt = self.data.read(buf)?;
    self.trace.lock().unwrap().accept(buf, amt, self.got);

    self.got += amt;
    Ok(amt)
  }
}

#[cfg(test)]
mod test {
  use super::JsonLoad;
  use crate::Load;

  #[test]
  fn test_json() {
    let doc = r#"
{
  "version": "1.2.3"
}"#;

    let marked_data = JsonLoad::new("version").read(doc.to_string(), None).unwrap();
    assert_eq!("1.2.3", marked_data.value());
    assert_eq!(17, marked_data.start());
  }

  #[test]
  fn test_json_seq() {
    let doc = r#"
[
  "thing",
  [
    "thing2",
    "1.2.3"
  ]
]"#;

    let marked_data = JsonLoad::new("1.1").read(doc.to_string(), None).unwrap();
    assert_eq!("1.2.3", marked_data.value());
    assert_eq!(37, marked_data.start());
  }

  #[test]
  fn test_json_complex() {
    let doc = r#"
{
  "version": {
    "thing": [
      "2.4.6",
      { "version": "1.2.3" }
    ]
  }
}"#;

    let marked_data = JsonLoad::new("version.thing.1.version").read(doc.to_string(), None).unwrap();
    assert_eq!("1.2.3", marked_data.value());
    assert_eq!(68, marked_data.start());
  }

  #[test]
  fn test_json_utf8() {
    let doc = r#"
[
  "thíng",
  [
    "thíng2",
    "1.2.3"
  ]
]"#;

    let marked_data = JsonLoad::new("1.1").read(doc.to_string(), None).unwrap();
    assert_eq!("1.2.3", marked_data.value());
    assert_eq!(39, marked_data.start());
  }
}
