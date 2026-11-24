//! Locations of RuDo's files following the
//! [XDG base directory specification](https://specifications.freedesktop.org/basedir-spec/latest/).
//!
//! Resolution is split into pure functions that take the environment values as parameters
//! ([`BaseDir::resolve`], [`expand_tilde`]) and thin wrappers that read the real process
//! environment ([`BaseDir::app_dir`], [`home_dir`]).

use std::borrow::Cow;
use std::env;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// Name of RuDo's subdirectory inside every base directory.
pub const APP_DIR_NAME: &str = "rudo";

/// Errors that occur while locating RuDo's directories.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum PathsError {
    /// The home directory is unknown, or it is not an absolute path.
    #[error("cannot determine the home directory")]
    HomeNotFound,
}

/// One of the XDG base directories RuDo uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::EnumIter)]
pub enum BaseDir {
    /// User specific configuration (`$XDG_CONFIG_HOME`, default `~/.config`).
    Config,
    /// User specific data (`$XDG_DATA_HOME`, default `~/.local/share`).
    Data,
    /// User specific state such as logs (`$XDG_STATE_HOME`, default `~/.local/state`).
    State,
}

impl BaseDir {
    /// Returns the name of the environment variable that overrides this directory.
    #[must_use]
    pub const fn env_var(self) -> &'static str {
        match self {
            Self::Config => "XDG_CONFIG_HOME",
            Self::Data => "XDG_DATA_HOME",
            Self::State => "XDG_STATE_HOME",
        }
    }

    /// Returns the default location of this directory relative to the home directory.
    #[must_use]
    pub const fn home_relative_default(self) -> &'static str {
        match self {
            Self::Config => ".config",
            Self::Data => ".local/share",
            Self::State => ".local/state",
        }
    }

    /// Resolves this base directory from the value of its environment variable and the home
    /// directory, without reading the process environment.
    ///
    /// `xdg_value` is used when it is an absolute path. A missing, empty or relative value is
    /// ignored as the XDG specification demands, and the default below `home` is returned.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::ffi::OsStr;
    /// use std::path::Path;
    ///
    /// use rudo::paths::BaseDir;
    ///
    /// let home = Path::new("/home/alice");
    /// assert_eq!(BaseDir::State.resolve(None, home), Path::new("/home/alice/.local/state"));
    /// assert_eq!(
    ///     BaseDir::Config.resolve(Some(OsStr::new("/etc/xdg")), home),
    ///     Path::new("/etc/xdg"),
    /// );
    /// ```
    #[must_use]
    pub fn resolve(self, xdg_value: Option<&OsStr>, home: &Path) -> PathBuf {
        xdg_value
            .map(Path::new)
            .filter(|path| path.is_absolute())
            .map_or_else(
                || home.join(self.home_relative_default()),
                Path::to_path_buf,
            )
    }

    /// Returns RuDo's subdirectory ([`APP_DIR_NAME`]) of this base directory, resolved from
    /// the process environment. The directory is not created.
    ///
    /// # Errors
    ///
    /// Returns [`PathsError::HomeNotFound`] if the home directory cannot be determined.
    pub fn app_dir(self) -> Result<PathBuf, PathsError> {
        let home = home_dir()?;
        let xdg_value = env::var_os(self.env_var());
        Ok(self.resolve(xdg_value.as_deref(), &home).join(APP_DIR_NAME))
    }
}

/// Returns the current user's home directory.
///
/// # Errors
///
/// Returns [`PathsError::HomeNotFound`] if the home directory is unknown or not absolute.
pub fn home_dir() -> Result<PathBuf, PathsError> {
    env::home_dir()
        .filter(|home| home.is_absolute())
        .ok_or(PathsError::HomeNotFound)
}

/// Replaces a leading `~` in `path` with `home`.
///
/// Only `~` on its own and paths starting with `~/` are expanded. `~user/...` (the home
/// directory of another user) is not supported and, like every other path, returned
/// unchanged without allocating.
///
/// # Examples
///
/// ```
/// use std::path::Path;
///
/// use rudo::paths::expand_tilde;
///
/// let home = Path::new("/home/alice");
/// assert_eq!(expand_tilde(Path::new("~/todo"), home), Path::new("/home/alice/todo"));
/// assert_eq!(expand_tilde(Path::new("~bob/todo"), home), Path::new("~bob/todo"));
/// ```
#[must_use]
pub fn expand_tilde<'a>(path: &'a Path, home: &Path) -> Cow<'a, Path> {
    match path.strip_prefix("~") {
        Ok(rest) if rest.as_os_str().is_empty() => Cow::Owned(home.to_path_buf()),
        Ok(rest) => Cow::Owned(home.join(rest)),
        Err(_) => Cow::Borrowed(path),
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use strum::IntoEnumIterator;

    use super::*;

    const HOME: &str = "/home/alice";

    #[rstest]
    #[case::config_unset(BaseDir::Config, None, "/home/alice/.config")]
    #[case::config_empty(BaseDir::Config, Some(""), "/home/alice/.config")]
    #[case::config_relative(BaseDir::Config, Some("relative/config"), "/home/alice/.config")]
    #[case::config_absolute(BaseDir::Config, Some("/xdg/config"), "/xdg/config")]
    #[case::data_unset(BaseDir::Data, None, "/home/alice/.local/share")]
    #[case::data_empty(BaseDir::Data, Some(""), "/home/alice/.local/share")]
    #[case::data_relative(BaseDir::Data, Some("./data"), "/home/alice/.local/share")]
    #[case::data_absolute(BaseDir::Data, Some("/xdg/data"), "/xdg/data")]
    #[case::state_unset(BaseDir::State, None, "/home/alice/.local/state")]
    #[case::state_empty(BaseDir::State, Some(""), "/home/alice/.local/state")]
    #[case::state_relative(BaseDir::State, Some("~/state"), "/home/alice/.local/state")]
    #[case::state_absolute(BaseDir::State, Some("/xdg/state"), "/xdg/state")]
    fn resolves_base_directory(
        #[case] base_dir: BaseDir,
        #[case] xdg_value: Option<&str>,
        #[case] expected: &str,
    ) {
        let xdg_value = xdg_value.map(OsStr::new);
        let home = Path::new(HOME);
        let expected = Path::new(expected);

        let system_under_test = base_dir.resolve(xdg_value, home);

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::config(BaseDir::Config, "XDG_CONFIG_HOME")]
    #[case::data(BaseDir::Data, "XDG_DATA_HOME")]
    #[case::state(BaseDir::State, "XDG_STATE_HOME")]
    fn names_environment_variable(#[case] base_dir: BaseDir, #[case] expected: &str) {
        let system_under_test = base_dir.env_var();

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn app_dirs_are_absolute_and_named_after_the_app() {
        let expected_name = OsStr::new(APP_DIR_NAME);

        let system_under_test: Vec<PathBuf> = BaseDir::iter()
            .map(|base_dir| base_dir.app_dir().unwrap())
            .collect();

        assert!(system_under_test.iter().all(|dir| dir.is_absolute()));
        assert!(
            system_under_test
                .iter()
                .all(|dir| dir.file_name() == Some(expected_name))
        );
    }

    #[test]
    fn home_dir_is_absolute() {
        let system_under_test = home_dir().unwrap();

        assert!(system_under_test.is_absolute());
    }

    #[rstest]
    #[case::tilde_alone("~", "/home/alice")]
    #[case::tilde_slash("~/", "/home/alice")]
    #[case::tilde_with_file("~/todo.md", "/home/alice/todo.md")]
    #[case::tilde_with_nested_path("~/Obsidian/Vault/Todo", "/home/alice/Obsidian/Vault/Todo")]
    #[case::other_user("~bob/todo", "~bob/todo")]
    #[case::tilde_in_middle("/srv/~/todo", "/srv/~/todo")]
    #[case::tilde_suffix("todo~", "todo~")]
    #[case::absolute("/srv/todo", "/srv/todo")]
    #[case::relative("todo", "todo")]
    #[case::empty("", "")]
    fn expands_tilde(#[case] path: &str, #[case] expected: &str) {
        let path = Path::new(path);
        let home = Path::new(HOME);
        let expected = Path::new(expected);

        let system_under_test = expand_tilde(path, home);

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::other_user("~bob/todo")]
    #[case::absolute("/srv/todo")]
    fn borrows_paths_without_tilde(#[case] path: &str) {
        let path = Path::new(path);
        let home = Path::new(HOME);

        let system_under_test = expand_tilde(path, home);

        assert!(matches!(system_under_test, Cow::Borrowed(_)));
    }
}
