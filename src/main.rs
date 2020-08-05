//! Versio is a version management utility.

use versio::error::Result;

fn main() {
  if let Err(e) = run() {
    use std::io::Write;
    let stderr = &mut std::io::stderr();
    let errmsg = "Error writing to stderr";

    writeln!(stderr, "error: {}", e).expect(errmsg);

    for e in e.iter().skip(1) {
      writeln!(stderr, "caused by: {}", e).expect(errmsg);
    }

    // Try running with `RUST_BACKTRACE=1` for a backtrace
    if let Some(backtrace) = e.backtrace() {
      writeln!(stderr, "backtrace: {:?}", backtrace).expect(errmsg);
    }

    std::process::exit(1);
  }
}

fn run() -> Result<()> {
  env_logger::try_init()?;
  versio::opts::execute()
}
