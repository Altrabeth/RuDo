//! The `rudo` binary: parses the command line, sets up logging and runs RuDo.

use anyhow::Context;
use clap::Parser;
use rudo::cli::Cli;
use rudo::logging::{self, LOG_FILE_NAME};
use rudo::paths::BaseDir;

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let log_file = BaseDir::State
        .app_dir()
        .context("cannot locate the state directory")?
        .join(LOG_FILE_NAME);
    logging::init(&log_file).context("cannot set up logging")?;
    tracing::info!(version = env!("CARGO_PKG_VERSION"), ?cli, "rudo started");
    Ok(())
}
