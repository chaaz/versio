//! Versio is a version management utility.

mod cli;

use tokio::runtime::Runtime;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};
use versio::commands::early_info;
use versio::errors::Result;

fn main() {
  if let Err(e) = Runtime::new().unwrap().block_on(run()) {
    use std::io::Write;
    let stderr = &mut std::io::stderr();

    writeln!(stderr, "Error: {:?}", e).expect("Error writing to stderr.");
    std::process::exit(1);
  }
}

async fn run() -> Result<()> {
  let format = fmt::format()
    .with_level(true)
    .with_target(false)
    .with_thread_ids(false)
    .with_thread_names(false)
    .with_source_location(false)
    .pretty()
    .with_source_location(false);

  tracing_subscriber::registry().with(fmt::layer().event_format(format)).with(EnvFilter::from_default_env()).init();

  let info = early_info()?;
  std::env::set_current_dir(info.working_dir())?;
  cli::execute(&info).await
}
