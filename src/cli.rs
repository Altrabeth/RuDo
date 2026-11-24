//! Command line interface definition.

use std::path::{Path, PathBuf};

use clap::Parser;

/// Keyboard-driven terminal UI for todo lists stored as Markdown files.
#[derive(Debug, Clone, PartialEq, Eq, Default, Parser)]
#[command(name = "rudo", version)]
pub struct Cli {
    /// Workspace folder (overrides `workspace.root`)
    #[arg(short, long, value_name = "DIR")]
    root: Option<PathBuf>,

    /// Configuration file (default: ~/.config/rudo/config.yaml)
    #[arg(short, long, value_name = "FILE")]
    config: Option<PathBuf>,
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
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;
    use clap::error::ErrorKind;
    use rstest::rstest;

    use super::*;

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
    fn stops_parsing_with_kind(#[case] arguments: &[&str], #[case] expected_kind: ErrorKind) {
        let system_under_test = Cli::try_parse_from(arguments).unwrap_err();

        assert_eq!(system_under_test.kind(), expected_kind);
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
