//! The `rudo` binary: parses the command line, sets up logging, loads the configuration and
//! the keymap and either runs a `config` subcommand or chooses the workspace folder.

use std::env;
use std::fmt;
use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;

use anyhow::Context;
use clap::Parser;
use rudo::cli::{Cli, Command, ConfigCommand};
use rudo::config::root::WorkspaceRoot;
use rudo::config::settings::Settings;
use rudo::config::theme::ColourMode;
use rudo::config::{self, CONFIG_FILE_NAMES, DEFAULT_CONFIG_YAML};
use rudo::logging::{self, LOG_FILE_NAME};
use rudo::paths::{self, BaseDir};

fn main() -> anyhow::Result<ExitCode> {
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
    let colour_mode = ColourMode::from_no_color(env::var_os("NO_COLOR").as_deref());
    match cli.subcommand() {
        Some(Command::Config(ConfigCommand::Path)) => {
            let path = config_file.unwrap_or_else(|| config_dir.join(CONFIG_FILE_NAMES[0]));
            write_output(io::stdout().lock(), format_args!("{}\n", path.display()))?;
            Ok(ExitCode::SUCCESS)
        }
        Some(Command::Config(ConfigCommand::Default)) => {
            write_output(io::stdout().lock(), format_args!("{DEFAULT_CONFIG_YAML}"))?;
            Ok(ExitCode::SUCCESS)
        }
        Some(Command::Config(ConfigCommand::Check)) => {
            check_config(config_file.as_deref(), &home, colour_mode)
        }
        None => {
            run(&cli, config_file.as_deref(), &home, colour_mode)?;
            Ok(ExitCode::SUCCESS)
        }
    }
}

/// Starts RuDo on the workspace folder.
fn run(
    cli: &Cli,
    config_file: Option<&Path>,
    home: &Path,
    colour_mode: ColourMode,
) -> anyhow::Result<()> {
    let settings =
        Settings::load(config_file, home, colour_mode).context("cannot load the configuration")?;
    tracing::info!(?config_file, ?settings, "configuration loaded");

    let default_root = BaseDir::Data
        .app_dir()
        .context("cannot locate the data directory")?;
    let root = WorkspaceRoot::select(
        cli.root(),
        settings.config().workspace().root(),
        &default_root,
        home,
    );
    root.ensure_exists()
        .context("cannot open the workspace folder")?;
    tracing::info!(?root, "workspace root resolved");
    Ok(())
}

/// Runs `rudo config check`: reports success on stdout, or every problem on stderr together
/// with a failing exit code.
fn check_config(
    config_file: Option<&Path>,
    home: &Path,
    colour_mode: ColourMode,
) -> anyhow::Result<ExitCode> {
    match Settings::load(config_file, home, colour_mode) {
        Ok(_) => {
            let mut stdout = io::stdout().lock();
            match config_file {
                Some(path) => write_output(
                    &mut stdout,
                    format_args!("configuration is valid: {}\n", path.display()),
                ),
                None => write_output(
                    &mut stdout,
                    format_args!(
                        "configuration is valid: no configuration file, using the defaults\n"
                    ),
                ),
            }?;
            Ok(ExitCode::SUCCESS)
        }
        Err(error) => {
            let error = anyhow::Error::new(error);
            tracing::info!(?config_file, "configuration check failed: {error:#}");
            write_output(io::stderr().lock(), format_args!("{error:#}\n"))?;
            Ok(ExitCode::FAILURE)
        }
    }
}

/// Writes program output (not logging) to `writer` and flushes it.
///
/// A closed pipe, as in `rudo config default | head -1`, means that the reader has all it
/// wants, so `BrokenPipe` counts as success; `println!` would panic instead.
fn write_output(mut writer: impl Write, output: fmt::Arguments) -> io::Result<()> {
    match writer.write_fmt(output).and_then(|()| writer.flush()) {
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        result => result,
    }
}
