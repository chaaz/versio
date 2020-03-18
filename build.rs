use std::io::{Error, ErrorKind};
use std::process::Command;

fn main() {
  // You need to touch (or clean) build.rs to rebuild if any of the lalrpop or git rev changes.
  // println!("cargo:rerun-if-changed=build.rs");

  let output = Command::new("git").args(&["rev-parse", "--short", "HEAD"]).output();
  let git_hash = output.and_then(|output| String::from_utf8(output.stdout).map_err(conv_err));
  git_hash
    .or_else(|_| Ok::<_, Error>(String::from("-------")))
    .map(|hash| println!("cargo:rustc-env=GIT_SHORT_HASH={}", hash))
    .unwrap()
}

fn err(msg: String) -> Error { Error::new(ErrorKind::Other, msg) }

fn conv_err<E: ::std::error::Error>(e: E) -> Error { err(e.to_string()) }
