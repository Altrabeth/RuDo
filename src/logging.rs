//! Logging to a file.
//!
//! RuDo never logs to stdout or stderr because that would corrupt the terminal interface.
//! Log events go to a file instead, filtered by the directives in [`LOG_ENV_VAR`].

use std::env::{self, VarError};
use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tracing::Subscriber;
use tracing::dispatcher::SetGlobalDefaultError;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::filter::{LevelFilter, ParseError};
use tracing_subscriber::fmt::MakeWriter;

/// Environment variable holding the
/// [`tracing` filter directives](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html),
/// for example `debug` or `rudo=trace`.
pub const LOG_ENV_VAR: &str = "RUDO_LOG";

/// Name of the log file inside RuDo's state directory.
pub const LOG_FILE_NAME: &str = "rudo.log";

/// Level used when [`LOG_ENV_VAR`] is unset or empty.
const DEFAULT_LEVEL: LevelFilter = LevelFilter::INFO;

/// Errors that occur while setting up logging.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum LoggingError {
    /// The directory of the log file could not be created.
    #[error("cannot create log directory {}", path.display())]
    CreateDirectory {
        /// Directory that could not be created.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: io::Error,
    },
    /// The log file could not be opened for appending.
    #[error("cannot open log file {}", path.display())]
    OpenFile {
        /// File that could not be opened.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: io::Error,
    },
    /// [`LOG_ENV_VAR`] is not valid Unicode.
    #[error("{LOG_ENV_VAR} is not valid unicode")]
    NonUnicodeFilter,
    /// [`LOG_ENV_VAR`] contains an invalid filter directive.
    #[error("invalid filter in {LOG_ENV_VAR}")]
    InvalidFilter(#[source] ParseError),
    /// Another global logger was installed before.
    #[error("a global logger is already installed")]
    AlreadyInitialized(#[source] SetGlobalDefaultError),
}

/// Installs the global logger, which appends events to `log_file` without ANSI escape codes.
///
/// The log file and its parent directories are created if they do not exist. The level is
/// read from [`LOG_ENV_VAR`] and defaults to `info`.
///
/// # Errors
///
/// Returns [`LoggingError::NonUnicodeFilter`] or [`LoggingError::InvalidFilter`] if
/// [`LOG_ENV_VAR`] cannot be parsed, [`LoggingError::CreateDirectory`] or
/// [`LoggingError::OpenFile`] if the log file cannot be opened, and
/// [`LoggingError::AlreadyInitialized`] if a global logger is already installed.
pub fn init(log_file: &Path) -> Result<(), LoggingError> {
    let filter = env_filter(&filter_directives()?)?;
    let file = open_log_file(log_file)?;
    tracing::subscriber::set_global_default(file_subscriber(filter, Mutex::new(file)))
        .map_err(LoggingError::AlreadyInitialized)
}

fn filter_directives() -> Result<String, LoggingError> {
    match env::var(LOG_ENV_VAR) {
        Ok(directives) => Ok(directives),
        Err(VarError::NotPresent) => Ok(String::new()),
        Err(VarError::NotUnicode(_)) => Err(LoggingError::NonUnicodeFilter),
    }
}

fn env_filter(directives: &str) -> Result<EnvFilter, LoggingError> {
    EnvFilter::builder()
        .with_default_directive(DEFAULT_LEVEL.into())
        .parse(directives)
        .map_err(LoggingError::InvalidFilter)
}

fn open_log_file(path: &Path) -> Result<File, LoggingError> {
    if let Some(directory) = path.parent() {
        fs::create_dir_all(directory).map_err(|source| LoggingError::CreateDirectory {
            path: directory.to_path_buf(),
            source,
        })?;
    }
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|source| LoggingError::OpenFile {
            path: path.to_path_buf(),
            source,
        })
}

fn file_subscriber<W>(filter: EnvFilter, writer: W) -> impl Subscriber + Send + Sync + 'static
where
    W: for<'writer> MakeWriter<'writer> + Send + Sync + 'static,
{
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_ansi(false)
        .with_writer(writer)
        .finish()
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use tempfile::TempDir;

    use super::*;

    /// Runs `emit_events` with a file subscriber using `directives` and returns the log text.
    fn log_output(directives: &str, emit_events: impl FnOnce()) -> String {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join(LOG_FILE_NAME);
        let file = open_log_file(&path).unwrap();
        let subscriber = file_subscriber(env_filter(directives).unwrap(), Mutex::new(file));
        tracing::subscriber::with_default(subscriber, emit_events);
        fs::read_to_string(path).unwrap()
    }

    #[rstest]
    #[case::empty_uses_default("", LevelFilter::INFO)]
    #[case::global_level("debug", LevelFilter::DEBUG)]
    #[case::quieter_level("warn", LevelFilter::WARN)]
    #[case::target_level("rudo=trace", LevelFilter::TRACE)]
    fn parses_filter_directives(#[case] directives: &str, #[case] expected: LevelFilter) {
        let system_under_test = env_filter(directives).unwrap();

        assert_eq!(system_under_test.max_level_hint(), Some(expected));
    }

    #[test]
    fn rejects_invalid_filter_directives() {
        let directives = "rudo=loud";

        let system_under_test = env_filter(directives);

        assert!(matches!(
            system_under_test,
            Err(LoggingError::InvalidFilter(_))
        ));
    }

    #[test]
    fn open_log_file_creates_missing_directories() {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("state/rudo").join(LOG_FILE_NAME);

        let system_under_test = open_log_file(&path);

        assert!(system_under_test.is_ok());
        assert!(path.is_file());
    }

    #[test]
    fn open_log_file_appends_to_existing_content() {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join(LOG_FILE_NAME);
        let existing_content = "earlier run\n";
        fs::write(&path, existing_content).unwrap();
        let appended_content = "later run\n";
        let expected = "earlier run\nlater run\n";

        let system_under_test = {
            let mut file = open_log_file(&path).unwrap();
            io::Write::write_all(&mut file, appended_content.as_bytes()).unwrap();
            fs::read_to_string(&path).unwrap()
        };

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn open_log_file_reports_uncreatable_directory() {
        let directory = TempDir::new().unwrap();
        let blocking_file = directory.path().join("not-a-directory");
        fs::write(&blocking_file, "").unwrap();
        let path = blocking_file.join(LOG_FILE_NAME);

        let system_under_test = open_log_file(&path);

        assert!(matches!(
            system_under_test,
            Err(LoggingError::CreateDirectory { path, .. }) if path == blocking_file
        ));
    }

    #[test]
    fn open_log_file_reports_unopenable_file() {
        let directory = TempDir::new().unwrap();
        let path = directory.path().to_path_buf();

        let system_under_test = open_log_file(&path);

        assert!(matches!(
            system_under_test,
            Err(LoggingError::OpenFile { path: failed, .. }) if failed == path
        ));
    }

    #[test]
    fn writes_events_without_ansi_escape_codes() {
        let directives = "info";
        let message = "rudo started";
        let escape_character = '\u{1b}';

        let system_under_test = log_output(directives, || tracing::info!("{message}"));

        assert!(system_under_test.contains(message));
        assert!(system_under_test.contains("INFO"));
        assert!(!system_under_test.contains(escape_character));
    }

    #[test]
    fn drops_events_below_the_filter_level() {
        let directives = "";
        let message = "noisy details";

        let system_under_test = log_output(directives, || tracing::debug!("{message}"));

        assert_eq!(system_under_test, "");
    }
}
