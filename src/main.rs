//! The `rudo` binary: parses the command line, sets up logging, loads the configuration and
//! chooses the workspace folder.

use anyhow::Context;
use clap::Parser;
use rudo::cli::Cli;
use rudo::config::root::WorkspaceRoot;
use rudo::config::{self, Config};
use rudo::logging::{self, LOG_FILE_NAME};
use rudo::paths::{self, BaseDir};

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let log_file = BaseDir::State
        .app_dir()
        .context("cannot locate the state directory")?
        .join(LOG_FILE_NAME);
    logging::init(&log_file).context("cannot set up logging")?;
    tracing::info!(version = env!("CARGO_PKG_VERSION"), ?cli, "rudo started");

    let home = paths::home_dir()?;
    let config_dir = BaseDir::Config
        .app_dir()
        .context("cannot locate the configuration directory")?;
    let config_file = config::find_config_file(cli.config(), &config_dir);
    let config = match &config_file {
        Some(path) => Config::load(path, &home).context("cannot load the configuration")?,
        None => Config::default(),
    };
    tracing::info!(?config_file, ?config, "configuration loaded");

    let default_root = BaseDir::Data
        .app_dir()
        .context("cannot locate the data directory")?;
    let root = WorkspaceRoot::select(cli.root(), config.workspace().root(), &default_root, &home);
    root.ensure_exists()
        .context("cannot open the workspace folder")?;
    tracing::info!(?root, "workspace root resolved");
    Ok(())
}
