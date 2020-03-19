//! Utilities to find a mark in a TOML file.

use crate::error::Result;
#[cfg(test)]
use crate::parts::ToPart;
use crate::parts::{IntoPartVec, Part};
use crate::{Mark, MarkedData, NamedData, Scanner};
use serde::de::{self, DeserializeSeed, Deserializer, IgnoredAny, MapAccess, SeqAccess, Unexpected, Visitor};
use toml::Spanned;

pub struct TomlScanner {
  target: Vec<Part>
}

impl TomlScanner {
  pub fn new<P: IntoPartVec>(target: P) -> TomlScanner { TomlScanner { target: target.into_part_vec() } }

  #[cfg(test)]
  pub fn from_parts(target: &[&dyn ToPart]) -> TomlScanner { TomlScanner { target: target.into_part_vec() } }
}

impl Scanner for TomlScanner {
  fn scan(&self, data: NamedData) -> Result<MarkedData> {
    let byte_mark = scan_toml(data.data(), self.target.clone())?;
    Ok(data.mark(byte_mark))
  }
}

fn scan_toml<P: IntoPartVec>(data: &str, loc: P) -> Result<Mark> {
  let mut parts = loc.into_part_vec();
  parts.reverse();

  let value = pop(parts).deserialize(&mut toml::Deserializer::new(data))?;
  let index = value.span().0;

  // TODO: handle triple quotes
  Ok(Mark::make(value.into_inner(), index + 1)?)
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
        let nth = if self.remains.is_empty() {
          map.next_value()?
        } else {
          let next = pop(std::mem::replace(&mut self.remains, Vec::new()));
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
      seq.next_element()?.ok_or_else(|| de::Error::invalid_length(n, &self))?
    } else {
      let next = pop(std::mem::replace(&mut self.remains, Vec::new()));
      seq.next_element_seed(next)?.ok_or_else(|| de::Error::invalid_length(n, &self))?
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
  use super::TomlScanner;
  use crate::{NamedData, Scanner};

  #[test]
  fn test_toml() {
    let doc = r#"
version = "1.2.3""#;

    let marked_data = TomlScanner::new("version").scan(NamedData::new(None, doc.to_string())).unwrap();
    assert_eq!("1.2.3", marked_data.value());
    assert_eq!(12, marked_data.start());
  }

  #[test]
  fn test_toml_seq() {
    let doc = r#"
thing = [ "thing2", "1.2.3" ]"#;

    let marked_data = TomlScanner::new("thing.1").scan(NamedData::new(None, doc.to_string())).unwrap();
    assert_eq!("1.2.3", marked_data.value());
    assert_eq!(22, marked_data.start());
  }

  #[test]
  fn test_toml_complex() {
    let doc = r#"
[version]
"thing" = [ "2.4.6", { "version" = "1.2.3" } ]"#;

    let marked_data = TomlScanner::new("version.thing.1.version").scan(NamedData::new(None, doc.to_string())).unwrap();
    assert_eq!("1.2.3", marked_data.value());
    assert_eq!(47, marked_data.start());
  }

  #[test]
  fn test_toml_clever() {
    let doc = r#"
[[0]]
"the.version" = "1.2.3""#;

    let marked_data =
      TomlScanner::from_parts(&[&"0", &0, &"the.version"]).scan(NamedData::new(None, doc.to_string())).unwrap();
    assert_eq!("1.2.3", marked_data.value());
    assert_eq!(24, marked_data.start());
  }

  #[test]
  fn test_toml_utf8() {
    let doc = r#"
"thíng" = [ "thíng2", "1.2.3" ]"#;

    let marked_data = TomlScanner::new("thíng.1").scan(NamedData::new(None, doc.to_string())).unwrap();
    assert_eq!("1.2.3", marked_data.value());
    assert_eq!(26, marked_data.start());
  }
}
