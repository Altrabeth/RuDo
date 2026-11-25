//! The configuration file: lookup, parsing into [`Config`] and error reporting.
//!
//! Every section is optional and every missing setting falls back to its default, so a
//! configuration file only needs the settings a user wants to change. Unknown settings are
//! errors that name the file, line and column.

pub mod keymap;
pub mod root;
pub mod settings;
pub mod theme;

use std::fmt;
use std::fs;
use std::io;
use std::num::NonZeroU8;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde::de::{self, Deserializer, Unexpected, Visitor};
use serde_saphyr::{UserMessageFormatter, render_options};

use self::theme::ThemeConfig;
use crate::config::keymap::keymap::KeyBindings;
use crate::paths::expand_tilde;
use crate::storage::bin::BinPath;

/// The complete default configuration with comments, as printed by `rudo config default`.
pub const DEFAULT_CONFIG_YAML: &str = include_str!("../../assets/default-config.yaml");

/// Names of the configuration file inside RuDo's configuration directory, in lookup order.
pub const CONFIG_FILE_NAMES: [&str; 2] = ["config.yaml", "config.yml"];

/// Errors that occur while loading the configuration file.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ConfigError {
    /// The configuration file does not exist.
    #[error("configuration file {} does not exist", path.display())]
    NotFound {
        /// File that was looked for.
        path: PathBuf,
    },
    /// The configuration file exists but could not be read.
    #[error("cannot read configuration file {}", path.display())]
    Read {
        /// File that could not be read.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: io::Error,
    },
    /// The configuration file is not valid YAML or does not match the expected settings.
    #[error("invalid configuration file {}\n{report}", path.display())]
    Invalid {
        /// File that contains the error.
        path: PathBuf,
        /// Description of the error with line, column and the offending snippet.
        report: String,
    },
}

/// RuDo's settings, read from the configuration file and completed with defaults.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    #[serde(deserialize_with = "null_as_default")]
    workspace: WorkspaceConfig,
    #[serde(deserialize_with = "null_as_default")]
    tasks: TasksConfig,
    editor: Option<String>,
    #[serde(deserialize_with = "null_as_default")]
    theme: ThemeConfig,
    #[serde(deserialize_with = "null_as_default")]
    keybindings: KeyBindings,
}

impl Config {
    /// Reads the configuration file at `path`.
    ///
    /// An empty file yields the defaults. A leading `~` in `workspace.root` is replaced with
    /// `home`, and a relative `workspace.root` is made relative to the directory of `path`.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::NotFound`] if `path` does not exist, [`ConfigError::Read`] if
    /// it cannot be read and [`ConfigError::Invalid`] if its content is not a valid
    /// configuration.
    pub fn load(path: &Path, home: &Path) -> Result<Self, ConfigError> {
        let yaml = fs::read_to_string(path).map_err(|source| match source.kind() {
            io::ErrorKind::NotFound => ConfigError::NotFound {
                path: path.to_path_buf(),
            },
            _ => ConfigError::Read {
                path: path.to_path_buf(),
                source,
            },
        })?;
        let mut config = Self::from_yaml(&yaml, path)?;
        let config_dir = path.parent().unwrap_or(Path::new(""));
        config.workspace.root = config
            .workspace
            .root
            .map(|root| resolve_configured_root(&root, config_dir, home));
        Ok(config)
    }

    /// Parses the configuration in `yaml` without resolving any paths. `path` only names the
    /// source in error messages.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Invalid`] if `yaml` is not a valid configuration.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::path::Path;
    ///
    /// use rudo::config::{Config, IndentStyle};
    ///
    /// let config = Config::from_yaml("tasks:\n  indent: tab\n", Path::new("config.yaml"))?;
    /// assert_eq!(config.tasks().indent(), IndentStyle::Tab);
    /// assert!(Config::from_yaml("tasks:\n  indent: 9\n", Path::new("config.yaml")).is_err());
    /// # Ok::<(), rudo::config::ConfigError>(())
    /// ```
    pub fn from_yaml(yaml: &str, path: &Path) -> Result<Self, ConfigError> {
        let options = serde_saphyr::options! { strict_booleans: true };
        serde_saphyr::from_str_with_options(yaml, options).map_err(|error| {
            let source_name = path.to_string_lossy();
            let report = error.render_with_options(render_options! {
                formatter: &UserMessageFormatter,
                source_name: Some(&source_name),
            });
            ConfigError::Invalid {
                path: path.to_path_buf(),
                report,
            }
        })
    }

    /// Returns the `workspace` section.
    #[must_use]
    pub fn workspace(&self) -> &WorkspaceConfig {
        &self.workspace
    }

    /// Returns the `tasks` section.
    #[must_use]
    pub fn tasks(&self) -> &TasksConfig {
        &self.tasks
    }

    /// Returns the command used to open a list in an editor, if one is configured.
    #[must_use]
    pub fn editor(&self) -> Option<&str> {
        self.editor.as_deref()
    }

    /// Returns the `theme` section, from which [`Theme::new`](theme::Theme::new) builds the
    /// effective theme.
    #[must_use]
    pub fn theme(&self) -> &ThemeConfig {
        &self.theme
    }

    /// Returns the `keybindings` section: the keys that replace the defaults of actions.
    #[must_use]
    pub fn keybindings(&self) -> &KeyBindings {
        &self.keybindings
    }
}

/// Deserializes a section, reading null as the default section: a section whose entries are
/// all commented out is null in YAML, and it means "change nothing".
fn null_as_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    Option::<T>::deserialize(deserializer).map(Option::unwrap_or_default)
}

/// The `workspace` section of the configuration.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WorkspaceConfig {
    root: Option<PathBuf>,
    show_hidden: bool,
    #[serde(deserialize_with = "null_as_default")]
    bin: BinPath,
}

impl WorkspaceConfig {
    /// Returns the configured workspace folder, if any.
    #[must_use]
    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    /// Returns whether hidden folders and lists, whose name starts with `.`, are shown on
    /// start-up.
    #[must_use]
    pub const fn show_hidden(&self) -> bool {
        self.show_hidden
    }

    /// Returns the folder inside the workspace that receives deleted lists and folders.
    #[must_use]
    pub fn bin(&self) -> &BinPath {
        &self.bin
    }
}

/// The `tasks` section of the configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TasksConfig {
    show_completed: bool,
    record_completion_date: bool,
    indent: IndentStyle,
}

impl Default for TasksConfig {
    fn default() -> Self {
        Self {
            show_completed: true,
            record_completion_date: true,
            indent: IndentStyle::Tab,
        }
    }
}

impl TasksConfig {
    /// Returns whether completed tasks are shown on start-up.
    #[must_use]
    pub const fn show_completed(&self) -> bool {
        self.show_completed
    }

    /// Returns whether a completion date is written when a task is marked done.
    #[must_use]
    pub const fn record_completion_date(&self) -> bool {
        self.record_completion_date
    }

    /// Returns the indentation for subtasks in files that have none yet.
    #[must_use]
    pub const fn indent(&self) -> IndentStyle {
        self.indent
    }
}

/// Indentation of subtasks: a tab or a number of spaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum IndentStyle {
    /// One tab per level, written as `tab` in the configuration.
    #[default]
    Tab,
    /// A number of spaces per level, written as an integer in the configuration.
    Spaces(IndentWidth),
}

/// Number of spaces per indentation level, from 1 to [`IndentWidth::MAX`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IndentWidth(NonZeroU8);

impl IndentWidth {
    /// Largest supported number of spaces per indentation level.
    pub const MAX: u8 = 8;

    /// Returns the width `spaces`, or `None` if it is not between 1 and [`Self::MAX`].
    #[must_use]
    pub fn new(spaces: u8) -> Option<Self> {
        NonZeroU8::new(spaces)
            .filter(|spaces| spaces.get() <= Self::MAX)
            .map(Self)
    }

    /// Returns the number of spaces.
    #[must_use]
    pub const fn get(self) -> u8 {
        self.0.get()
    }
}

impl<'de> Deserialize<'de> for IndentStyle {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(IndentStyleVisitor)
    }
}

struct IndentStyleVisitor;

impl Visitor<'_> for IndentStyleVisitor {
    type Value = IndentStyle;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        write!(
            formatter,
            "`tab` or a number of spaces from 1 to {}",
            IndentWidth::MAX
        )
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
        match value {
            "tab" => Ok(IndentStyle::Tab),
            _ => Err(E::invalid_value(Unexpected::Str(value), &self)),
        }
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<Self::Value, E> {
        u8::try_from(value)
            .ok()
            .and_then(IndentWidth::new)
            .map(IndentStyle::Spaces)
            .ok_or_else(|| E::invalid_value(Unexpected::Unsigned(value), &self))
    }

    fn visit_i64<E: de::Error>(self, value: i64) -> Result<Self::Value, E> {
        u64::try_from(value)
            .map_err(|_| E::invalid_value(Unexpected::Signed(value), &self))
            .and_then(|value| self.visit_u64(value))
    }
}

/// Returns the configuration file to load: `explicit` if given, otherwise the first of
/// [`CONFIG_FILE_NAMES`] that exists in `config_dir`, otherwise `None` (use the defaults).
///
/// `explicit` is returned even if it does not exist, so that [`Config::load`] reports it.
#[must_use]
pub fn find_config_file(explicit: Option<&Path>, config_dir: &Path) -> Option<PathBuf> {
    explicit.map(Path::to_path_buf).or_else(|| {
        CONFIG_FILE_NAMES
            .iter()
            .map(|name| config_dir.join(name))
            .find(|path| path.is_file())
    })
}

fn resolve_configured_root(root: &Path, config_dir: &Path, home: &Path) -> PathBuf {
    let root = expand_tilde(root, home);
    if root.is_absolute() {
        root.into_owned()
    } else {
        config_dir.join(root)
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use tempfile::TempDir;

    use super::*;
    use crate::config::keymap::action::Action;
    use crate::config::keymap::keymap::Keymap;
    use crate::readme;

    const HOME: &str = "/home/alice";

    fn source() -> &'static Path {
        Path::new("/etc/rudo/config.yaml")
    }

    fn write_file(directory: &TempDir, name: &str, content: &str) -> PathBuf {
        let path = directory.path().join(name);
        fs::write(&path, content).unwrap();
        path
    }

    fn report(error: ConfigError) -> String {
        match error {
            ConfigError::Invalid { report, .. } => report,
            other => panic!("expected an invalid configuration, got {other:?}"),
        }
    }

    #[test]
    fn default_config_file_matches_default_config() {
        let expected = Config::default();

        let system_under_test = Config::from_yaml(DEFAULT_CONFIG_YAML, source()).unwrap();

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn defaults_match_the_documented_values() {
        let expected_tasks = TasksConfig {
            show_completed: true,
            record_completion_date: true,
            indent: IndentStyle::Tab,
        };

        let system_under_test = Config::default();

        assert_eq!(system_under_test.workspace().root(), None);
        assert!(!system_under_test.workspace().show_hidden());
        assert_eq!(system_under_test.workspace().bin(), &BinPath::default());
        assert_eq!(*system_under_test.tasks(), expected_tasks);
        assert_eq!(system_under_test.editor(), None);
    }

    #[rstest]
    #[case::empty("")]
    #[case::only_whitespace("\n\n  \n")]
    #[case::only_comments("# nothing to change\n")]
    fn empty_file_means_defaults(#[case] yaml: &str) {
        let expected = Config::default();

        let system_under_test = Config::from_yaml(yaml, source()).unwrap();

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::workspace("workspace:\n  # root: ~/todo\n")]
    #[case::tasks("tasks:\n  # indent: 2\n")]
    #[case::theme("theme:\n  # tag: { fg: red }\n")]
    #[case::keybindings("keybindings:\n  # quit: [q]\n")]
    #[case::explicit_null("tasks: null\n")]
    #[case::tilde("theme: ~\n")]
    fn null_section_means_defaults(#[case] yaml: &str) {
        let expected = Config::default();

        let system_under_test = Config::from_yaml(yaml, source()).unwrap();

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn partial_configuration_keeps_other_defaults() {
        let yaml = "tasks:\n  show_completed: false\neditor: nvim\n";
        let expected = Config {
            tasks: TasksConfig {
                show_completed: false,
                ..TasksConfig::default()
            },
            editor: Some("nvim".to_owned()),
            ..Config::default()
        };

        let system_under_test = Config::from_yaml(yaml, source()).unwrap();

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn reads_every_setting() {
        let yaml = "workspace:\n  root: /srv/todo\n  show_hidden: true\n  bin: Bin\ntasks:\n  \
                    show_completed: false\n  record_completion_date: false\n  indent: 2\n\
                    editor: code --wait\n";
        let expected = Config {
            workspace: WorkspaceConfig {
                root: Some(PathBuf::from("/srv/todo")),
                show_hidden: true,
                bin: BinPath::try_from(PathBuf::from("Bin")).unwrap(),
            },
            tasks: TasksConfig {
                show_completed: false,
                record_completion_date: false,
                indent: IndentStyle::Spaces(IndentWidth::new(2).unwrap()),
            },
            editor: Some("code --wait".to_owned()),
            theme: ThemeConfig::default(),
            keybindings: KeyBindings::default(),
        };

        let system_under_test = Config::from_yaml(yaml, source()).unwrap();

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn tilde_root_is_null() {
        let yaml = "workspace:\n  root: ~\n";

        let system_under_test = Config::from_yaml(yaml, source()).unwrap();

        assert_eq!(system_under_test.workspace().root(), None);
    }

    #[test]
    fn reads_keybindings() {
        let yaml = "keybindings:\n  toggle-done: [x, space, ctrl-x]\n  set-due-date: \"t d\"\n  \
                    open-in-editor: []\n";
        let expected: KeyBindings = [
            (Action::ToggleDone, &["x", "space", "ctrl-x"][..]),
            (Action::SetDueDate, &["t d"]),
            (Action::OpenInEditor, &[]),
        ]
        .into_iter()
        .map(|(action, notations)| {
            let keys = notations.iter().map(|key| key.parse().unwrap()).collect();
            (action, keys)
        })
        .collect();

        let system_under_test = Config::from_yaml(yaml, source()).unwrap();

        assert_eq!(*system_under_test.keybindings(), expected);
    }

    #[test]
    fn unknown_action_is_reported_with_location_and_valid_names() {
        let yaml = "keybindings:\n  teleport: t\n";
        let expected_location = "/etc/rudo/config.yaml:2:3";
        let expected_names = "expected one of help, quit";

        let system_under_test = report(Config::from_yaml(yaml, source()).unwrap_err());

        assert!(
            system_under_test.contains(expected_location),
            "{system_under_test}"
        );
        assert!(
            system_under_test.contains(expected_names),
            "{system_under_test}"
        );
    }

    #[test]
    fn default_config_file_has_a_valid_keymap() {
        let config = Config::from_yaml(DEFAULT_CONFIG_YAML, source()).unwrap();

        let system_under_test = Keymap::new(config.keybindings());

        assert_eq!(system_under_test.err(), None);
    }

    #[test]
    fn readme_configuration_example_has_a_valid_keymap() {
        let example: String = readme::section("## Configuration")
            .into_iter()
            .skip_while(|line| *line != "```yaml")
            .skip(1)
            .take_while(|line| *line != "```")
            .flat_map(|line| [line, "\n"])
            .collect();
        let config = Config::from_yaml(&example, source()).unwrap();

        let system_under_test = Keymap::new(config.keybindings());

        assert_eq!(system_under_test.err(), None);
    }

    #[rstest]
    #[case::top_level("theme_name: dark\n", "theme_name", 1, 1)]
    #[case::workspace("workspace:\n  folder: /srv\n", "folder", 2, 3)]
    #[case::tasks("tasks:\n  show_complete: true\n", "show_complete", 2, 3)]
    fn unknown_key_is_reported_with_location(
        #[case] yaml: &str,
        #[case] unknown_key: &str,
        #[case] line: u32,
        #[case] column: u32,
    ) {
        let expected_location = format!("/etc/rudo/config.yaml:{line}:{column}");
        let expected_message = format!("unknown field `{unknown_key}`");

        let system_under_test = report(Config::from_yaml(yaml, source()).unwrap_err());

        assert!(
            system_under_test.contains(&expected_location),
            "{system_under_test}"
        );
        assert!(
            system_under_test.contains(&expected_message),
            "{system_under_test}"
        );
    }

    #[rstest]
    #[case::number_for_boolean("tasks:\n  show_completed: 3\n", "2:19")]
    #[case::yes_for_boolean("tasks:\n  show_completed: yes\n", "2:19")]
    #[case::off_for_boolean("tasks:\n  record_completion_date: off\n", "2:27")]
    #[case::list_for_section("tasks: [true]\n", "1:8")]
    #[case::list_for_editor("editor: [nvim]\n", "1:9")]
    fn wrong_type_is_reported_with_location(#[case] yaml: &str, #[case] location: &str) {
        let expected_location = format!("/etc/rudo/config.yaml:{location}");

        let system_under_test = report(Config::from_yaml(yaml, source()).unwrap_err());

        assert!(
            system_under_test.contains(&expected_location),
            "{system_under_test}"
        );
    }

    #[rstest]
    #[case::absolute("/srv/trash", "relative to the workspace folder")]
    #[case::outside("../trash", "inside the workspace folder")]
    #[case::root(".", "not be the workspace folder itself")]
    fn invalid_bin_is_reported_with_location(#[case] bin: &str, #[case] expected_hint: &str) {
        let yaml = format!("workspace:\n  bin: {bin}\n");

        let system_under_test = report(Config::from_yaml(&yaml, source()).unwrap_err());

        assert!(
            system_under_test.contains(expected_hint),
            "{system_under_test}"
        );
        assert!(
            system_under_test.contains("config.yaml:2:"),
            "{system_under_test}"
        );
    }

    #[rstest]
    #[case::yes("yes")]
    #[case::on("on")]
    fn yaml_1_1_boolean_explains_expected_values(#[case] value: &str) {
        let yaml = format!("tasks:\n  show_completed: {value}\n");
        let expected_hint = "true or false expected";

        let system_under_test = report(Config::from_yaml(&yaml, source()).unwrap_err());

        assert!(
            system_under_test.contains(expected_hint),
            "{system_under_test}"
        );
    }

    #[test]
    fn error_shows_the_offending_snippet() {
        let yaml = "tasks:\n  indent: 9\n";
        let expected_snippet = "2 |   indent: 9";

        let system_under_test = report(Config::from_yaml(yaml, source()).unwrap_err());

        assert!(
            system_under_test.contains(expected_snippet),
            "{system_under_test}"
        );
    }

    #[test]
    fn invalid_yaml_syntax_is_reported_with_location() {
        let yaml = "tasks:\n  indent: [tab\n";
        let expected_file = "/etc/rudo/config.yaml:";

        let system_under_test = report(Config::from_yaml(yaml, source()).unwrap_err());

        assert!(
            system_under_test.contains(expected_file),
            "{system_under_test}"
        );
    }

    #[rstest]
    #[case::tab("tab", IndentStyle::Tab)]
    #[case::one_space("1", IndentStyle::Spaces(IndentWidth::new(1).unwrap()))]
    #[case::four_spaces("4", IndentStyle::Spaces(IndentWidth::new(4).unwrap()))]
    #[case::eight_spaces("8", IndentStyle::Spaces(IndentWidth::new(8).unwrap()))]
    fn reads_valid_indent(#[case] value: &str, #[case] expected: IndentStyle) {
        let yaml = format!("tasks:\n  indent: {value}\n");

        let system_under_test = Config::from_yaml(&yaml, source()).unwrap();

        assert_eq!(system_under_test.tasks().indent(), expected);
    }

    #[rstest]
    #[case::zero("0")]
    #[case::nine("9")]
    #[case::beyond_u8("300")]
    #[case::negative("-2")]
    #[case::fraction("2.5")]
    #[case::other_word("spaces")]
    #[case::capitalised_tab("Tab")]
    #[case::quoted_number("\"4\"")]
    #[case::boolean("true")]
    fn rejects_invalid_indent(#[case] value: &str) {
        let yaml = format!("tasks:\n  indent: {value}\n");
        let expected_hint = "`tab` or a number of spaces from 1 to 8";

        let system_under_test = report(Config::from_yaml(&yaml, source()).unwrap_err());

        assert!(
            system_under_test.contains(expected_hint),
            "{system_under_test}"
        );
        assert!(
            system_under_test.contains("config.yaml:2:3"),
            "{system_under_test}"
        );
    }

    #[rstest]
    #[case::zero(0, None)]
    #[case::one(1, Some(1))]
    #[case::max(8, Some(8))]
    #[case::too_wide(9, None)]
    #[case::largest_u8(u8::MAX, None)]
    fn indent_width_accepts_one_to_eight(#[case] spaces: u8, #[case] expected: Option<u8>) {
        let system_under_test = IndentWidth::new(spaces);

        assert_eq!(system_under_test.map(IndentWidth::get), expected);
    }

    #[rstest]
    #[case::absolute("/srv/todo", "/srv/todo")]
    #[case::tilde("~/Obsidian/Todo", "/home/alice/Obsidian/Todo")]
    #[case::tilde_alone("~", "/home/alice")]
    #[case::relative("todo", "/etc/rudo/todo")]
    #[case::relative_parent("../todo", "/etc/rudo/../todo")]
    fn resolves_configured_root(#[case] root: &str, #[case] expected: &str) {
        let root = Path::new(root);
        let config_dir = Path::new("/etc/rudo");
        let home = Path::new(HOME);
        let expected = Path::new(expected);

        let system_under_test = resolve_configured_root(root, config_dir, home);

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn load_resolves_relative_root_against_the_file_directory() {
        let directory = TempDir::new().unwrap();
        let path = write_file(&directory, "config.yaml", "workspace:\n  root: lists\n");
        let home = Path::new(HOME);
        let expected = directory.path().join("lists");

        let system_under_test = Config::load(&path, home).unwrap();

        assert_eq!(
            system_under_test.workspace().root(),
            Some(expected.as_path())
        );
    }

    #[test]
    fn load_expands_tilde_in_root() {
        let directory = TempDir::new().unwrap();
        let path = write_file(&directory, "config.yaml", "workspace:\n  root: ~/todo\n");
        let home = Path::new(HOME);
        let expected = Path::new("/home/alice/todo");

        let system_under_test = Config::load(&path, home).unwrap();

        assert_eq!(system_under_test.workspace().root(), Some(expected));
    }

    #[test]
    fn load_reads_empty_file_as_defaults() {
        let directory = TempDir::new().unwrap();
        let path = write_file(&directory, "config.yaml", "");
        let home = Path::new(HOME);
        let expected = Config::default();

        let system_under_test = Config::load(&path, home).unwrap();

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn load_reports_missing_file() {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("missing.yaml");
        let home = Path::new(HOME);

        let system_under_test = Config::load(&path, home);

        assert!(matches!(
            system_under_test,
            Err(ConfigError::NotFound { path: missing }) if missing == path
        ));
    }

    #[test]
    fn load_reports_unreadable_file() {
        let directory = TempDir::new().unwrap();
        let path = directory.path().to_path_buf();
        let home = Path::new(HOME);

        let system_under_test = Config::load(&path, home);

        assert!(matches!(
            system_under_test,
            Err(ConfigError::Read { path: unreadable, .. }) if unreadable == path
        ));
    }

    #[test]
    fn load_names_the_file_in_errors() {
        let directory = TempDir::new().unwrap();
        let path = write_file(&directory, "config.yaml", "editor: nvim\ncolour: red\n");
        let home = Path::new(HOME);
        let expected_location = format!("{}:2:1", path.display());

        let system_under_test = Config::load(&path, home).unwrap_err().to_string();

        assert!(
            system_under_test.contains(&expected_location),
            "{system_under_test}"
        );
    }

    #[rstest]
    #[case::none_exists(&[], None)]
    #[case::only_yaml(&["config.yaml"], Some("config.yaml"))]
    #[case::only_yml(&["config.yml"], Some("config.yml"))]
    #[case::yaml_before_yml(&["config.yml", "config.yaml"], Some("config.yaml"))]
    #[case::other_names_ignored(&["rudo.yaml", "config.json"], None)]
    fn finds_config_file_in_config_dir(
        #[case] existing_files: &[&str],
        #[case] expected_name: Option<&str>,
    ) {
        let directory = TempDir::new().unwrap();
        for name in existing_files {
            write_file(&directory, name, "");
        }
        let expected = expected_name.map(|name| directory.path().join(name));

        let system_under_test = find_config_file(None, directory.path());

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn explicit_config_file_wins_even_if_missing() {
        let directory = TempDir::new().unwrap();
        write_file(&directory, "config.yaml", "");
        let explicit = Path::new("missing.yaml");
        let expected = Some(PathBuf::from("missing.yaml"));

        let system_under_test = find_config_file(Some(explicit), directory.path());

        assert_eq!(system_under_test, expected);
    }
}
