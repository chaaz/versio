//! Utilities to find a mark in a JSON file.

use crate::error::Result;
#[cfg(test)]
use crate::scan::parts::ToPart;
use crate::scan::parts::{IntoPartVec, Part};
use crate::scan::Scanner;
use crate::{Mark, MarkedData, NamedData};
use serde::de::{self, DeserializeSeed, Deserializer, IgnoredAny, MapAccess, SeqAccess, Unexpected, Visitor};
use std::sync::{Arc, Mutex};

type TraceRef = Arc<Mutex<Trace>>;

pub struct JsonScanner {
  target: Vec<Part>
}

impl JsonScanner {
  pub fn new<P: IntoPartVec>(target: P) -> JsonScanner { JsonScanner { target: target.into_part_vec() } }

  #[cfg(test)]
  pub fn from_parts(target: &[&dyn ToPart]) -> JsonScanner { JsonScanner { target: target.into_part_vec() } }
}

impl Scanner for JsonScanner {
  fn scan(&self, data: NamedData) -> Result<MarkedData> {
    let byte_mark = scan_json(&data.data(), self.target.clone())?;
    Ok(data.mark(byte_mark))
  }
}

fn scan_json<P: IntoPartVec>(data: &str, loc: P) -> Result<Mark> {
  let mut parts = loc.into_part_vec();
  parts.reverse();

  let trace = Arc::new(Mutex::new(Trace::new()));
  let reader = MeteredReader::new(data.as_bytes(), trace.clone());

  let value = pop(parts, trace.clone()).deserialize(&mut serde_json::Deserializer::from_reader(reader))?;
  let index = trace.lock()?.find_start()?;

  Ok(Mark::make(value, index)?)
}

fn pop(mut parts: Vec<Part>, trace: TraceRef) -> NthElement {
  let part = parts.pop().unwrap();
  NthElement::new(part, parts, trace)
}

struct NthElement {
  part: Part,
  remains: Vec<Part>,
  trace: TraceRef
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
        let nth = if self.remains.is_empty() {
          self.trace.lock().unwrap().set_active(true);
          let r = map.next_value()?;
          self.trace.lock().unwrap().set_active(false);
          r
        } else {
          let next = pop(std::mem::replace(&mut self.remains, Vec::new()), self.trace.clone());
          map.next_value_seed(next)?
        };

        got_val = Some(nth);
        break;
      } else {
        map.next_value::<IgnoredAny>()?;
      }
    }

    while let Some((IgnoredAny, IgnoredAny)) = map.next_entry()? {}

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

    let nth = if self.remains.is_empty() {
      self.trace.lock().unwrap().set_active(true);
      let r = seq.next_element()?.ok_or_else(|| de::Error::invalid_length(n, &self))?;
      self.trace.lock().unwrap().set_active(false);
      r
    } else {
      let next = pop(std::mem::replace(&mut self.remains, Vec::new()), self.trace.clone());
      seq.next_element_seed(next)?.ok_or_else(|| de::Error::invalid_length(n, &self))?
    };

    while let Some(IgnoredAny) = seq.next_element()? {}

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

struct Trace {
  active: bool,
  leader: usize,
  bytes: Vec<u8>
}

impl Default for Trace {
  fn default() -> Trace { Trace::new() }
}

impl Trace {
  pub fn new() -> Trace { Trace { active: false, leader: 0, bytes: Vec::new() } }

  pub fn set_active(&mut self, active: bool) { self.active = active; }

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
        + self.leader
        + 1
    )
  }
}

struct MeteredReader<'a> {
  data: &'a [u8],
  got: usize,
  trace: TraceRef
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
  use super::JsonScanner;
  use crate::{scan::Scanner, NamedData};

  #[test]
  fn test_json() {
    let doc = r#"
{
  "version": "1.2.3"
}"#;

    let marked_data = JsonScanner::new("version").scan(NamedData::new(None, doc.to_string())).unwrap();
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

    let marked_data = JsonScanner::new("1.1").scan(NamedData::new(None, doc.to_string())).unwrap();
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

    let marked_data = JsonScanner::new("version.thing.1.version").scan(NamedData::new(None, doc.to_string())).unwrap();
    assert_eq!("1.2.3", marked_data.value());
    assert_eq!(68, marked_data.start());
  }

  #[test]
  fn test_json_clever() {
    let doc = r#"
{
  "outer": {
    "0": [
      { "the.version": "1.2.3" }
    ]
  }
}"#;

    let marked_data = JsonScanner::from_parts(&[&"outer", &"0", &0, &"the.version"])
      .scan(NamedData::new(None, doc.to_string()))
      .unwrap();
    assert_eq!("1.2.3", marked_data.value());
    assert_eq!(51, marked_data.start());
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

    let marked_data = JsonScanner::new("1.1").scan(NamedData::new(None, doc.to_string())).unwrap();
    assert_eq!("1.2.3", marked_data.value());
    assert_eq!(39, marked_data.start());
  }
}
