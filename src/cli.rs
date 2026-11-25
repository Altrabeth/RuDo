//! Command line interface definition and the output of the subcommands.

use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};
use jiff::tz::TimeZone;

use crate::storage::bin::BinEntry;
use crate::storage::workspace::Entry;

/// Shown instead of the deletion time of an entry that another program put into the bin,
/// padded to the width of a time.
const UNKNOWN_TIME: &str = "unknown         ";

/// Keyboard-driven terminal UI for todo lists stored as Markdown files.
#[derive(Debug, Clone, PartialEq, Eq, Default, Parser)]
#[command(name = "rudo", version)]
pub struct Cli {
    /// Workspace folder (overrides `workspace.root`)
    #[arg(short, long, value_name = "DIR", global = true)]
    root: Option<PathBuf>,

    /// Configuration file (default: ~/.config/rudo/config.yaml)
    #[arg(short, long, value_name = "FILE", global = true)]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Command>,
}

/// A subcommand that runs instead of the terminal interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Subcommand)]
pub enum Command {
    /// Inspect the configuration file
    #[command(subcommand)]
    Config(ConfigCommand),
    /// Inspect or empty the bin of the workspace
    #[command(subcommand)]
    Bin(BinCommand),
}

/// A subcommand of `rudo config`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Subcommand)]
pub enum ConfigCommand {
    /// Print the path of the configuration file in use
    Path,
    /// Print the default configuration
    Default,
    /// Validate the configuration file
    Check,
}

/// A subcommand of `rudo bin`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Subcommand)]
pub enum BinCommand {
    /// List the lists and folders in the bin
    List,
    /// Delete everything in the bin permanently
    Empty {
        /// Do not ask before deleting
        #[arg(short, long)]
        yes: bool,
    },
}

impl Cli {
    /// Returns the workspace folder given with `--root`, if any.
    #[must_use]
    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    /// Returns the configuration file given with `--config`, if any.
    #[must_use]
    pub fn config(&self) -> Option<&Path> {
        self.config.as_deref()
    }

    /// Returns the subcommand, or `None` if the terminal interface is to be started.
    #[must_use]
    pub const fn subcommand(&self) -> Option<Command> {
        self.command
    }
}

/// Returns the line of `rudo bin list` for `entry`: the deletion time in `time_zone`, the
/// original path (folders end with `/`) and where the entry is in the bin if that differs, or
/// `unknown` and its path in the bin for an entry that another program put there.
///
/// # Examples
///
/// ```
/// use jiff::tz::TimeZone;
/// use rudo::cli::bin_entry_line;
/// use rudo::storage::bin::scan_bin;
///
/// let root = tempfile::TempDir::new()?;
/// std::fs::create_dir(root.path().join(".trash"))?;
/// std::fs::write(root.path().join(".trash/Note.md"), "")?;
/// let entries = scan_bin(root.path(), &Default::default());
///
/// let line = bin_entry_line(&entries[0], &TimeZone::UTC);
/// assert_eq!(line, "unknown           Note.md (original path unknown)");
/// # Ok::<(), std::io::Error>(())
/// ```
#[must_use]
pub fn bin_entry_line(entry: &BinEntry, time_zone: &TimeZone) -> String {
    let time = entry.deleted_at().map_or_else(
        || UNKNOWN_TIME.to_owned(),
        |deleted_at| {
            deleted_at
                .to_zoned_datetime(time_zone.clone())
                .strftime("%Y-%m-%d %H:%M")
                .to_string()
        },
    );
    let folder_mark = match entry.entry() {
        Entry::Folder(_) => "/",
        Entry::List(_) => "",
    };
    let location = entry.location().display();
    match entry.original() {
        None => format!("{time}  {location}{folder_mark} (original path unknown)"),
        Some(original) if original == entry.location() => {
            format!("{time}  {}{folder_mark}", original.display())
        }
        Some(original) => format!(
            "{time}  {}{folder_mark} (in the bin as {location}{folder_mark})",
            original.display()
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use clap::CommandFactory;
    use clap::error::ErrorKind;
    use rstest::rstest;
    use tempfile::TempDir;

    use super::*;
    use crate::storage::bin::{BinPath, move_to_bin, scan_bin};

    #[test]
    fn command_definition_is_consistent() {
        let command = Cli::command();

        let system_under_test = command;

        system_under_test.debug_assert();
    }

    #[rstest]
    #[case::no_arguments(&["rudo"], None, None)]
    #[case::short_root(&["rudo", "-r", "/todo"], Some("/todo"), None)]
    #[case::long_root(&["rudo", "--root", "/todo"], Some("/todo"), None)]
    #[case::long_root_with_equals(&["rudo", "--root=~/todo"], Some("~/todo"), None)]
    #[case::short_config(&["rudo", "-c", "my.yaml"], None, Some("my.yaml"))]
    #[case::long_config(&["rudo", "--config", "my.yaml"], None, Some("my.yaml"))]
    #[case::both_short(&["rudo", "-r", "/todo", "-c", "my.yaml"], Some("/todo"), Some("my.yaml"))]
    #[case::both_long(
        &["rudo", "--config", "my.yaml", "--root", "/todo"],
        Some("/todo"),
        Some("my.yaml")
    )]
    fn parses_root_and_config(
        #[case] arguments: &[&str],
        #[case] expected_root: Option<&str>,
        #[case] expected_config: Option<&str>,
    ) {
        let expected = Cli {
            root: expected_root.map(PathBuf::from),
            config: expected_config.map(PathBuf::from),
            command: None,
        };

        let system_under_test = Cli::try_parse_from(arguments).unwrap();

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::short_help(&["rudo", "-h"], ErrorKind::DisplayHelp)]
    #[case::long_help(&["rudo", "--help"], ErrorKind::DisplayHelp)]
    #[case::short_version(&["rudo", "-V"], ErrorKind::DisplayVersion)]
    #[case::long_version(&["rudo", "--version"], ErrorKind::DisplayVersion)]
    #[case::root_without_value(&["rudo", "--root"], ErrorKind::InvalidValue)]
    #[case::unknown_flag(&["rudo", "--verbose"], ErrorKind::UnknownArgument)]
    #[case::unknown_command(&["rudo", "edit"], ErrorKind::InvalidSubcommand)]
    #[case::config_without_subcommand(
        &["rudo", "config"],
        ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
    )]
    #[case::unknown_config_subcommand(&["rudo", "config", "edit"], ErrorKind::InvalidSubcommand)]
    #[case::bin_without_subcommand(
        &["rudo", "bin"],
        ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
    )]
    #[case::unknown_bin_subcommand(&["rudo", "bin", "restore"], ErrorKind::InvalidSubcommand)]
    fn stops_parsing_with_kind(#[case] arguments: &[&str], #[case] expected_kind: ErrorKind) {
        let system_under_test = Cli::try_parse_from(arguments).unwrap_err();

        assert_eq!(system_under_test.kind(), expected_kind);
    }

    #[rstest]
    #[case::path(&["rudo", "config", "path"], ConfigCommand::Path)]
    #[case::default(&["rudo", "config", "default"], ConfigCommand::Default)]
    #[case::check(&["rudo", "config", "check"], ConfigCommand::Check)]
    fn parses_config_subcommands(
        #[case] arguments: &[&str],
        #[case] expected_command: ConfigCommand,
    ) {
        let expected = Some(Command::Config(expected_command));

        let system_under_test = Cli::try_parse_from(arguments).unwrap();

        assert_eq!(system_under_test.subcommand(), expected);
    }

    #[rstest]
    #[case::list(&["rudo", "bin", "list"], BinCommand::List)]
    #[case::empty(&["rudo", "bin", "empty"], BinCommand::Empty { yes: false })]
    #[case::empty_without_asking(&["rudo", "bin", "empty", "--yes"], BinCommand::Empty { yes: true })]
    #[case::empty_short(&["rudo", "bin", "empty", "-y"], BinCommand::Empty { yes: true })]
    fn parses_bin_subcommands(#[case] arguments: &[&str], #[case] expected_command: BinCommand) {
        let expected = Some(Command::Bin(expected_command));

        let system_under_test = Cli::try_parse_from(arguments).unwrap();

        assert_eq!(system_under_test.subcommand(), expected);
    }

    #[rstest]
    #[case::list("Home/Groceries.md", "Home/Groceries.md")]
    #[case::folder("Work", "Work/")]
    fn bin_entry_line_shows_the_time_and_the_original_path(
        #[case] path: &str,
        #[case] expected_path: &str,
    ) {
        let root = TempDir::new().unwrap();
        fs::create_dir_all(root.path().join("Home")).unwrap();
        fs::create_dir_all(root.path().join("Work")).unwrap();
        fs::write(root.path().join("Home/Groceries.md"), "").unwrap();
        move_to_bin(root.path(), &BinPath::default(), Path::new(path)).unwrap();
        let entries = scan_bin(root.path(), &BinPath::default());
        let time = entries[0]
            .deleted_at()
            .unwrap()
            .to_zoned_datetime(TimeZone::UTC)
            .strftime("%Y-%m-%d %H:%M")
            .to_string();
        let expected = format!("{time}  {expected_path}");

        let system_under_test = bin_entry_line(&entries[0], &TimeZone::UTC);

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn bin_entry_line_names_a_numbered_entry_by_its_place_in_the_bin() {
        let root = TempDir::new().unwrap();
        fs::create_dir_all(root.path().join(".trash")).unwrap();
        fs::write(root.path().join(".trash/Groceries.md"), "").unwrap();
        fs::write(root.path().join("Groceries.md"), "").unwrap();
        move_to_bin(root.path(), &BinPath::default(), Path::new("Groceries.md")).unwrap();
        let entries = scan_bin(root.path(), &BinPath::default());

        let system_under_test = bin_entry_line(&entries[0], &TimeZone::UTC);

        assert!(
            system_under_test.ends_with("  Groceries.md (in the bin as Groceries 2.md)"),
            "{system_under_test}"
        );
    }

    #[rstest]
    #[case::before_command(&["rudo", "-c", "my.yaml", "config", "check"])]
    #[case::after_command(&["rudo", "config", "check", "--config", "my.yaml"])]
    fn accepts_config_option_on_either_side_of_the_subcommand(#[case] arguments: &[&str]) {
        let expected_config = Path::new("my.yaml");
        let expected_command = Some(Command::Config(ConfigCommand::Check));

        let system_under_test = Cli::try_parse_from(arguments).unwrap();

        assert_eq!(system_under_test.config(), Some(expected_config));
        assert_eq!(system_under_test.subcommand(), expected_command);
    }

    #[test]
    fn accessors_return_parsed_paths() {
        let arguments = ["rudo", "-r", "/todo", "-c", "my.yaml"];
        let expected_root = Path::new("/todo");
        let expected_config = Path::new("my.yaml");

        let system_under_test = Cli::try_parse_from(arguments).unwrap();

        assert_eq!(system_under_test.root(), Some(expected_root));
        assert_eq!(system_under_test.config(), Some(expected_config));
    }
}
