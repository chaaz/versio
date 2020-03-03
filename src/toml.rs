//! Utilities to find a mark in a TOML file.

use crate::error::Result;
use crate::{Load, Mark, MarkedData};
use serde::de::{self, DeserializeSeed, Deserializer, IgnoredAny, MapAccess, SeqAccess, Unexpected, Visitor};
use std::fs::read_to_string;
use std::path::{Path, PathBuf};
use toml::Spanned;

pub struct TomlLoad {
  target: Vec<Part>
}

impl TomlLoad {
  pub fn new<P: IntoPartVec>(target: P) -> TomlLoad { TomlLoad { target: target.into_part_vec() } }
}

impl Load for TomlLoad {
  fn load<P: AsRef<Path>>(&self, filename: P) -> Result<MarkedData> {
    let data = read_to_string(&filename)?;
    self.read(data, Some(filename.as_ref().to_path_buf()))
  }

  fn read(&self, data: String, fname: Option<PathBuf>) -> Result<MarkedData> {
    let byte_mark = scan_toml(&data, self.target.clone())?;
    Ok(MarkedData::new(fname, data.to_string(), byte_mark))
  }
}

fn scan_toml<P: IntoPartVec>(data: &str, loc: P) -> Result<Mark> {
  let mut parts = loc.into_part_vec();
  parts.reverse();

  let value = pop(parts).deserialize(&mut toml::Deserializer::new(data))?;
  let index = value.span().0;

  // TODO: handle triple quotes
  Ok(Mark::new(value.into_inner(), index + 1))
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

fn pop(mut parts: Vec<Part>) -> NthElement {
  let part = parts.pop().unwrap();
  NthElement::new(part, parts)
}

pub struct NthElement {
  part: Part,
  remains: Vec<Part>
}

impl NthElement {
  pub fn new(part: Part, remains: Vec<Part>) -> NthElement { NthElement { part, remains } }
}

impl<'de> Visitor<'de> for NthElement {
  type Value = Spanned<String>;

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

    let mut got_val: Option<Spanned<String>> = None;

    while let Some(key) = map.next_key::<String>()? {
      if key == expected_key {
        let nth = match self.remains.is_empty() {
          true => {
            let r = map.next_value()?;
            r
          }
          false => {
            let next = pop(std::mem::replace(&mut self.remains, Vec::new()));
            map.next_value_seed(next)?
          }
        };

        got_val = Some(nth);
        break;
      } else {
        drop(map.next_value::<IgnoredAny>()?)
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

    let nth = match self.remains.is_empty() {
      true => {
        let r = seq.next_element()?.ok_or_else(|| de::Error::invalid_length(n, &self))?;
        r
      }
      false => {
        let next = pop(std::mem::replace(&mut self.remains, Vec::new()));
        seq.next_element_seed(next)?.ok_or_else(|| de::Error::invalid_length(n, &self))?
      }
    };

    while let Some(IgnoredAny) = seq.next_element()? {}

    Ok(nth)
  }
}

impl<'de> DeserializeSeed<'de> for NthElement {
  type Value = Spanned<String>;

  fn deserialize<D>(self, deserializer: D) -> std::result::Result<Self::Value, D::Error>
  where
    D: Deserializer<'de>
  {
    deserializer.deserialize_any(self)
  }
}

#[cfg(test)]
mod test {
  use super::TomlLoad;
  use crate::Load;

  #[test]
  fn test_toml() {
    let doc = r#"
version = "1.2.3""#;

    let marked_data = TomlLoad::new("version").read(doc.to_string(), None).unwrap();
    assert_eq!("1.2.3", marked_data.value());
    assert_eq!(12, marked_data.start());
  }

  #[test]
  fn test_toml_seq() {
    let doc = r#"
thing = [ "thing2", "1.2.3" ]"#;

    let marked_data = TomlLoad::new("thing.1").read(doc.to_string(), None).unwrap();
    assert_eq!("1.2.3", marked_data.value());
    assert_eq!(22, marked_data.start());
  }

  #[test]
  fn test_toml_complex() {
    let doc = r#"
[version]
"thing" = [ "2.4.6", { "version" = "1.2.3" } ]"#;

    let marked_data = TomlLoad::new("version.thing.1.version").read(doc.to_string(), None).unwrap();
    assert_eq!("1.2.3", marked_data.value());
    assert_eq!(47, marked_data.start());
  }

  #[test]
  fn test_toml_utf8() {
    let doc = r#"
"thíng" = [ "thíng2", "1.2.3" ]"#;

    let marked_data = TomlLoad::new("thíng.1").read(doc.to_string(), None).unwrap();
    assert_eq!("1.2.3", marked_data.value());
    assert_eq!(26, marked_data.start());
  }
}
