mod error;
mod yaml;

use crate::error::Result;
use crate::yaml::scan_yaml;

#[derive(Debug)]
pub struct Mark {
  value: String,
  char_start: usize,
}

impl Mark {
  pub fn new(value: String, char_start: usize) -> Mark {
    Mark { value, char_start }
  }

  pub fn value(&self) -> &str { &self.value }
  pub fn start(&self) -> usize { self.char_start }
}

fn main() -> Result<()> {
  let doc = r#"name: "bob"
thing:
  - first
  - second: 1
  - third: |
      yo
      ho
  - fourth: >
      hey
      yo
    other_x: 123
  - hmmm
  - version: 1.2.3
  - this is long"#;

  let value = scan_yaml(doc, "thing.5.version")?;
  println!("Got {:?}", value);

  Ok(())
}

