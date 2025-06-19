use std::io::Error;
use std::process::Command;

fn main() {
  // You'd need to touch (or clean) build.rs to rebuild if any of the lalrpop or git rev changes.
  // println!("cargo:rerun-if-changed=build.rs");

  // let git_output = Command::new("git").args(&["rev-parse", "--short", "HEAD"]).output();
  let git_output = Command::new("git").args(["describe", "--always", "--long", "--dirty"]).output();
  let git_hash = git_output.and_then(|output| String::from_utf8(output.stdout).map_err(conv_err));
  git_hash
    .or_else(|_| Ok::<_, Error>(String::from("-------")))
    .map(|hash| println!("cargo:rustc-env=GIT_SHORT_HASH={}", hash))
    .unwrap();

  let date_output = Command::new("date").args(["+%Y-%m-%d"]).output();
  let date_dash = date_output.and_then(|output| String::from_utf8(output.stdout).map_err(conv_err));
  date_dash
    .or_else(|_| Ok::<_, Error>(String::from("yy-mm-dd")))
    .map(|output| println!("cargo:rustc-env=DATE_DASH={}", output))
    .unwrap();
}

fn err(msg: String) -> Error { Error::other(msg) }

fn conv_err<E: ::std::error::Error>(e: E) -> Error { err(e.to_string()) }
