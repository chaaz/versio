//! Versio is a version management utility.

mod cli;

use env_logger::{Builder, Env};
use versio::commands::early_info;
use versio::errors::Result;
use tokio::runtime::Runtime;

fn main() {
  if let Err(e) = Runtime::new().unwrap().block_on(run()) {
    use std::io::Write;
    let stderr = &mut std::io::stderr();
    let errmsg = "Error writing to stderr.";

    writeln!(stderr, "Error: {}", e).expect(errmsg);

    for e in e.iter().skip(1) {
      writeln!(stderr, "  Caused by: {}", e).expect(errmsg);
    }

    // Try running with `RUST_BACKTRACE=1` for a backtrace
    if let Some(backtrace) = e.backtrace() {
      writeln!(stderr, "Backtrace:\n{:?}", backtrace).expect(errmsg);
    }

    std::process::exit(1);
  }
}

async fn run() -> Result<()> {
  // This is even better than `env_logger::try_init()?`.
  Builder::from_env(Env::new().default_filter_or("versio=warn")).try_init()?;

  let info = early_info()?;
  std::env::set_current_dir(info.working_dir())?;
  cli::execute(&info).await
}
