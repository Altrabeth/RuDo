//! Everything the configuration file controls, loaded and validated in one step.
//!
//! Both the terminal interface and `rudo config check` go through [`Settings::load`], so the
//! check reports exactly the problems that would stop RuDo from starting.

use std::fmt;
use std::path::Path;

use crate::config::keymap::keymap::{Keymap, KeymapError};
use crate::config::theme::{ColourMode, Theme};
use crate::config::{Config, ConfigError};

/// Why the configuration cannot be used.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SettingsError {
    /// The configuration file cannot be read or parsed.
    #[error(transparent)]
    Config(#[from] ConfigError),
    /// The keybindings contradict each other; every problem is listed on its own line.
    #[error("invalid keybindings{}", KeymapReport(.0))]
    Keymap(Vec<KeymapError>),
}

/// Lists every keymap error on its own line, so that all of them can be fixed in one go.
struct KeymapReport<'a>(&'a [KeymapError]);

impl fmt::Display for KeymapReport<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0
            .iter()
            .try_for_each(|error| write!(formatter, "\n  {error}"))
    }
}

/// The validated configuration together with the theme and keymap built from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    config: Config,
    theme: Theme,
    keymap: Keymap,
}

impl Settings {
    /// Loads the configuration file `config_file` (the defaults if `None`) and builds the
    /// theme for `colour_mode` and the keymap from it. `home` expands a leading `~` in
    /// `workspace.root`.
    ///
    /// # Errors
    ///
    /// Returns [`SettingsError::Config`] if the file cannot be loaded (see [`Config::load`])
    /// and [`SettingsError::Keymap`] with every conflict if the keybindings are invalid.
    pub fn load(
        config_file: Option<&Path>,
        home: &Path,
        colour_mode: ColourMode,
    ) -> Result<Self, SettingsError> {
        let config = match config_file {
            Some(path) => Config::load(path, home)?,
            None => Config::default(),
        };
        let keymap = Keymap::new(config.keybindings()).map_err(SettingsError::Keymap)?;
        let theme = Theme::new(config.theme(), colour_mode);
        Ok(Self {
            config,
            theme,
            keymap,
        })
    }

    /// Returns the configuration.
    #[must_use]
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Returns the effective theme.
    #[must_use]
    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    /// Returns the effective keymap.
    #[must_use]
    pub fn keymap(&self) -> &Keymap {
        &self.keymap
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;
    use crate::config::keymap::keymap::KeyBindings;

    const HOME: &str = "/home/alice";

    fn write_config(directory: &TempDir, yaml: &str) -> std::path::PathBuf {
        let path = directory.path().join("config.yaml");
        fs::write(&path, yaml).unwrap();
        path
    }

    #[test]
    fn load_without_file_uses_defaults() {
        let expected_config = Config::default();
        let expected_keymap = Keymap::new(&KeyBindings::default()).unwrap();
        let expected_theme = Theme::new(expected_config.theme(), ColourMode::Colour);

        let system_under_test = Settings::load(None, Path::new(HOME), ColourMode::Colour).unwrap();

        assert_eq!(*system_under_test.config(), expected_config);
        assert_eq!(*system_under_test.keymap(), expected_keymap);
        assert_eq!(*system_under_test.theme(), expected_theme);
    }

    #[test]
    fn load_builds_theme_for_colour_mode() {
        let directory = TempDir::new().unwrap();
        let path = write_config(&directory, "theme:\n  tag: { fg: red }\n");
        let config = Config::load(&path, Path::new(HOME)).unwrap();
        let expected_theme = Theme::new(config.theme(), ColourMode::NoColour);

        let system_under_test =
            Settings::load(Some(&path), Path::new(HOME), ColourMode::NoColour).unwrap();

        assert_eq!(*system_under_test.theme(), expected_theme);
        assert_eq!(*system_under_test.config(), config);
    }

    #[test]
    fn load_reports_missing_file() {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("missing.yaml");

        let system_under_test = Settings::load(Some(&path), Path::new(HOME), ColourMode::Colour);

        assert!(matches!(
            system_under_test,
            Err(SettingsError::Config(ConfigError::NotFound { .. }))
        ));
    }

    #[test]
    fn load_reports_yaml_error_with_line() {
        let directory = TempDir::new().unwrap();
        let path = write_config(&directory, "tasks:\n  indent: tab\n show_completed: true\n");
        let expected_fragment = "line 3";

        let system_under_test = Settings::load(Some(&path), Path::new(HOME), ColourMode::Colour)
            .unwrap_err()
            .to_string();

        assert!(
            system_under_test.contains(expected_fragment),
            "{system_under_test}"
        );
    }

    #[test]
    fn load_reports_every_keymap_conflict_on_its_own_line() {
        let directory = TempDir::new().unwrap();
        let path = write_config(&directory, "keybindings:\n  quit: [j]\n  help: [k]\n");
        let expected_lines = 3;
        let expected_actions = ["quit", "help", "move-down", "move-up"];

        let system_under_test = Settings::load(Some(&path), Path::new(HOME), ColourMode::Colour)
            .unwrap_err()
            .to_string();

        assert!(system_under_test.starts_with("invalid keybindings\n  "));
        assert!(system_under_test.lines().count() >= expected_lines);
        for action in expected_actions {
            assert!(system_under_test.contains(action), "{system_under_test}");
        }
    }
}
