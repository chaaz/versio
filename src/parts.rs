use serde::de::{self, Deserialize, Deserializer, SeqAccess, Visitor};
use std::fmt;

pub trait IntoPartVec {
  fn into_part_vec(self) -> Vec<Part>;
}

impl IntoPartVec for Vec<Part> {
  fn into_part_vec(self) -> Vec<Part> { self }
}

impl IntoPartVec for &str {
  fn into_part_vec(self) -> Vec<Part> { self.split('.').map(parse_part).collect() }
}

impl IntoPartVec for &[&dyn ToPart] {
  fn into_part_vec(self) -> Vec<Part> { self.iter().map(|d| d.to_part()).collect() }
}

pub fn parse_part(part: &str) -> Part {
  match part.parse() {
    Ok(i) => Part::Seq(i),
    Err(_) => Part::Map(part.to_string())
  }
}

pub trait ToPart {
  fn to_part(&self) -> Part;
}

impl ToPart for str {
  fn to_part(&self) -> Part { Part::Map(self.to_string()) }
}

impl ToPart for &str {
  fn to_part(&self) -> Part { (*self).to_part() }
}

impl ToPart for usize {
  fn to_part(&self) -> Part { Part::Seq(*self) }
}

#[derive(Clone, Debug)]
pub enum Part {
  Seq(usize),
  Map(String)
}

impl Part {
  pub fn seq_ind(&self) -> usize {
    match self {
      Part::Seq(i) => *i,
      _ => panic!("Part is not seq")
    }
  }
}

pub fn deserialize_parts<'de, D: Deserializer<'de>>(desr: D) -> std::result::Result<Vec<Part>, D::Error> {
  struct PartVecVisitor;

  impl<'de> Visitor<'de> for PartVecVisitor {
    type Value = Vec<Part>;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result { formatter.write_str("a parts target") }

    fn visit_str<E: de::Error>(self, v: &str) -> std::result::Result<Self::Value, E> { Ok(v.into_part_vec()) }

    fn visit_seq<V>(self, mut seq: V) -> std::result::Result<Self::Value, V::Error>
    where
      V: SeqAccess<'de>
    {
      let mut parts = Vec::new();
      while let Some(part) = seq.next_element()? {
        parts.push(part)
      }
      Ok(parts)
    }
  }

  desr.deserialize_any(PartVecVisitor)
}

impl<'de> Deserialize<'de> for Part {
  fn deserialize<D: Deserializer<'de>>(desr: D) -> std::result::Result<Part, D::Error> {
    struct PartVisitor;

    impl<'de> Visitor<'de> for PartVisitor {
      type Value = Part;

      fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result { formatter.write_str("a target part") }

      fn visit_i8<E: de::Error>(self, v: i8) -> std::result::Result<Self::Value, E> { Ok(Part::Seq(v as usize)) }
      fn visit_i16<E: de::Error>(self, v: i16) -> std::result::Result<Self::Value, E> { Ok(Part::Seq(v as usize)) }
      fn visit_i32<E: de::Error>(self, v: i32) -> std::result::Result<Self::Value, E> { Ok(Part::Seq(v as usize)) }
      fn visit_i64<E: de::Error>(self, v: i64) -> std::result::Result<Self::Value, E> { Ok(Part::Seq(v as usize)) }
      fn visit_i128<E: de::Error>(self, v: i128) -> std::result::Result<Self::Value, E> { Ok(Part::Seq(v as usize)) }
      fn visit_u8<E: de::Error>(self, v: u8) -> std::result::Result<Self::Value, E> { Ok(Part::Seq(v as usize)) }
      fn visit_u16<E: de::Error>(self, v: u16) -> std::result::Result<Self::Value, E> { Ok(Part::Seq(v as usize)) }
      fn visit_u32<E: de::Error>(self, v: u32) -> std::result::Result<Self::Value, E> { Ok(Part::Seq(v as usize)) }
      fn visit_u64<E: de::Error>(self, v: u64) -> std::result::Result<Self::Value, E> { Ok(Part::Seq(v as usize)) }
      fn visit_u128<E: de::Error>(self, v: u128) -> std::result::Result<Self::Value, E> { Ok(Part::Seq(v as usize)) }
      fn visit_f32<E: de::Error>(self, v: f32) -> std::result::Result<Self::Value, E> { Ok(Part::Seq(v as usize)) }
      fn visit_f64<E: de::Error>(self, v: f64) -> std::result::Result<Self::Value, E> { Ok(Part::Seq(v as usize)) }

      fn visit_str<E: de::Error>(self, v: &str) -> std::result::Result<Self::Value, E> { Ok(Part::Map(v.to_string())) }
    }

    desr.deserialize_any(PartVisitor)
  }
}
