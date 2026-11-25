//! Locations of RuDo's files on every [`Platform`], and the display of workspace paths.
//!
//! On Linux, macOS and other Unix systems RuDo follows the
//! [XDG base directory specification](https://specifications.freedesktop.org/basedir-spec/latest/).
//! On Windows it uses `%APPDATA%`, `%LOCALAPPDATA%` and the `Documents` folder, unless the
//! `XDG_*` variables are set.
//!
//! Resolution is split into pure functions that take the platform and the environment values
//! as parameters ([`BaseDir::resolve`], [`expand_tilde`]) and thin wrappers that read the real
//! process environment ([`BaseDir::app_dir`], [`home_dir`]).
//!
//! Paths relative to the workspace root are shown and stored with `/` between their
//! components on every platform ([`portable`]) and read with either separator
//! ([`parse_portable`]), so that a workspace synced between Windows and Linux reads the same.

use std::borrow::Cow;
use std::env;
use std::ffi::OsString;
use std::fmt::{self, Display, Formatter, Write as _};
use std::path::{Component, Path, PathBuf};

use crate::platform::Platform;

/// Name of RuDo's subdirectory inside every base directory.
pub const APP_DIR_NAME: &str = "rudo";

/// Name of the default workspace folder inside the `Documents` folder on Windows.
pub const WINDOWS_WORKSPACE_NAME: &str = "RuDo";

/// Errors that occur while locating RuDo's directories.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum PathsError {
    /// The home directory is unknown, or it is not an absolute path.
    #[error("cannot determine the home directory")]
    HomeNotFound,
}

/// One of the base directories RuDo uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::EnumIter)]
pub enum BaseDir {
    /// User specific configuration (`$XDG_CONFIG_HOME`, default `~/.config`, on Windows
    /// `%APPDATA%`).
    Config,
    /// User specific data, which holds the default workspace (`$XDG_DATA_HOME`, default
    /// `~/.local/share`, on Windows the `Documents` folder).
    Data,
    /// User specific state such as logs (`$XDG_STATE_HOME`, default `~/.local/state`, on
    /// Windows `%LOCALAPPDATA%`).
    State,
}

impl BaseDir {
    /// Returns the name of the XDG environment variable that overrides this directory on every
    /// platform.
    #[must_use]
    pub const fn env_var(self) -> &'static str {
        match self {
            Self::Config => "XDG_CONFIG_HOME",
            Self::Data => "XDG_DATA_HOME",
            Self::State => "XDG_STATE_HOME",
        }
    }

    /// Resolves RuDo's folder in this base directory on `platform` from the home directory and
    /// the environment, without reading the process environment.
    ///
    /// `environment` returns the value of an environment variable, `None` if it is unset. The
    /// XDG variable ([`BaseDir::env_var`]) wins on every platform when it holds an absolute
    /// path, which then gets the subdirectory [`APP_DIR_NAME`]; a missing, empty or relative
    /// value is ignored as the XDG specification demands. Otherwise the platform decides:
    ///
    /// - Linux, macOS and other Unix systems: `~/.config/rudo`, `~/.local/share/rudo` and
    ///   `~/.local/state/rudo`;
    /// - Windows: `%APPDATA%\rudo`, `~\Documents\RuDo` ([`WINDOWS_WORKSPACE_NAME`]) and
    ///   `%LOCALAPPDATA%\rudo`, where a missing, empty or relative `%APPDATA%` or
    ///   `%LOCALAPPDATA%` falls back to `~\AppData\Roaming` or `~\AppData\Local`.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::path::Path;
    ///
    /// use rudo::paths::BaseDir;
    /// use rudo::platform::Platform;
    ///
    /// let home = Path::new("/home/alice");
    /// assert_eq!(
    ///     BaseDir::State.resolve(Platform::Linux, home, |_| None),
    ///     Path::new("/home/alice/.local/state/rudo"),
    /// );
    /// assert_eq!(
    ///     BaseDir::Data.resolve(Platform::Windows, home, |_| None),
    ///     Path::new("/home/alice/Documents/RuDo"),
    /// );
    /// ```
    #[must_use]
    pub fn resolve(
        self,
        platform: Platform,
        home: &Path,
        environment: impl Fn(&str) -> Option<OsString>,
    ) -> PathBuf {
        absolute(environment(self.env_var())).map_or_else(
            || self.platform_default(platform, home, &environment),
            |xdg_dir| xdg_dir.join(APP_DIR_NAME),
        )
    }

    /// Returns RuDo's folder of this base directory, resolved for the current platform from
    /// the process environment. The directory is not created.
    ///
    /// # Errors
    ///
    /// Returns [`PathsError::HomeNotFound`] if the home directory cannot be determined.
    pub fn app_dir(self) -> Result<PathBuf, PathsError> {
        let home = home_dir()?;
        Ok(self.resolve(Platform::current(), &home, |name| env::var_os(name)))
    }

    /// Returns RuDo's folder of this base directory on `platform` when no XDG variable is set.
    fn platform_default(
        self,
        platform: Platform,
        home: &Path,
        environment: impl Fn(&str) -> Option<OsString>,
    ) -> PathBuf {
        match (platform, self) {
            (Platform::Linux | Platform::MacOs | Platform::OtherUnix, Self::Config) => {
                below(home, &[".config", APP_DIR_NAME])
            }
            (Platform::Linux | Platform::MacOs | Platform::OtherUnix, Self::Data) => {
                below(home, &[".local", "share", APP_DIR_NAME])
            }
            (Platform::Linux | Platform::MacOs | Platform::OtherUnix, Self::State) => {
                below(home, &[".local", "state", APP_DIR_NAME])
            }
            (Platform::Windows, Self::Config) => WindowsAppData::Roaming
                .resolve(home, environment)
                .join(APP_DIR_NAME),
            (Platform::Windows, Self::Data) => below(home, &["Documents", WINDOWS_WORKSPACE_NAME]),
            (Platform::Windows, Self::State) => WindowsAppData::Local
                .resolve(home, environment)
                .join(APP_DIR_NAME),
        }
    }
}

/// One of the two application data folders of a Windows user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WindowsAppData {
    /// `%APPDATA%`, which roams with the user's profile.
    Roaming,
    /// `%LOCALAPPDATA%`, which stays on the machine.
    Local,
}

impl WindowsAppData {
    /// Returns the folder from its environment variable if that holds an absolute path, else
    /// its usual place in the user's profile `home`, so that a stripped environment (services,
    /// some CI runners) still works.
    fn resolve(self, home: &Path, environment: impl Fn(&str) -> Option<OsString>) -> PathBuf {
        let (variable, folder) = match self {
            Self::Roaming => ("APPDATA", "Roaming"),
            Self::Local => ("LOCALAPPDATA", "Local"),
        };
        absolute(environment(variable)).unwrap_or_else(|| below(home, &["AppData", folder]))
    }
}

/// Returns `value` as a path if it is an absolute one.
fn absolute(value: Option<OsString>) -> Option<PathBuf> {
    value.map(PathBuf::from).filter(|path| path.is_absolute())
}

/// Returns the path of `components` below `base`, joined with the separator of the platform.
fn below(base: &Path, components: &[&str]) -> PathBuf {
    let mut path = base.to_path_buf();
    path.extend(components);
    path
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

/// A path displayed with `/` between its components on every platform, see [`portable`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PortablePath<'path>(&'path Path);

/// Displays `path`, relative to the workspace root, with `/` between its components on every
/// platform, so that the interface and the files RuDo writes look the same on Windows as on
/// Linux. Invalid Unicode is replaced as by [`Path::display`].
///
/// # Examples
///
/// ```
/// use std::path::PathBuf;
///
/// use rudo::paths::portable;
///
/// let path: PathBuf = ["Home", "Groceries.md"].iter().collect();
/// assert_eq!(portable(&path).to_string(), "Home/Groceries.md");
/// ```
#[must_use]
pub fn portable(path: &Path) -> PortablePath<'_> {
    PortablePath(path)
}

impl Display for PortablePath<'_> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        let mut separate = false;
        for component in self.0.components() {
            if separate {
                formatter.write_char('/')?;
            }
            match component {
                Component::RootDir => formatter.write_char('/')?,
                Component::Prefix(_)
                | Component::CurDir
                | Component::ParentDir
                | Component::Normal(_) => {
                    Path::new(component.as_os_str()).display().fmt(formatter)?;
                }
            }
            separate = !matches!(component, Component::Prefix(_) | Component::RootDir);
        }
        Ok(())
    }
}

/// Reads a path relative to the workspace root that a file stores, accepting both `/` and `\`
/// between its components whatever the platform, so that files written on Windows and on
/// Linux read the same. Empty components are dropped.
///
/// # Examples
///
/// ```
/// use std::path::PathBuf;
///
/// use rudo::paths::parse_portable;
///
/// let expected: PathBuf = ["Home", "Groceries.md"].iter().collect();
/// assert_eq!(parse_portable("Home/Groceries.md"), expected);
/// assert_eq!(parse_portable(r"Home\Groceries.md"), expected);
/// ```
#[must_use]
pub fn parse_portable(text: &str) -> PathBuf {
    text.split(['/', '\\'])
        .filter(|component| !component.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use strum::IntoEnumIterator;

    use super::*;

    const HOME: &str = "/home/alice";

    /// Returns the absolute path of `components` below the root of the file system, which
    /// needs a drive on Windows.
    fn absolute_path(components: &[&str]) -> PathBuf {
        let root = if cfg!(windows) { r"C:\" } else { "/" };
        below(Path::new(root), components)
    }

    fn home() -> PathBuf {
        absolute_path(&["home", "alice"])
    }

    /// Returns an environment that holds just `variables`.
    fn environment(variables: &[(&str, OsString)]) -> impl Fn(&str) -> Option<OsString> {
        move |name| {
            variables
                .iter()
                .find(|(variable, _)| *variable == name)
                .map(|(_, value)| value.clone())
        }
    }

    #[rstest]
    #[case::linux_config(Platform::Linux, BaseDir::Config, &[".config", "rudo"])]
    #[case::linux_data(Platform::Linux, BaseDir::Data, &[".local", "share", "rudo"])]
    #[case::linux_state(Platform::Linux, BaseDir::State, &[".local", "state", "rudo"])]
    #[case::macos_config(Platform::MacOs, BaseDir::Config, &[".config", "rudo"])]
    #[case::macos_data(Platform::MacOs, BaseDir::Data, &[".local", "share", "rudo"])]
    #[case::macos_state(Platform::MacOs, BaseDir::State, &[".local", "state", "rudo"])]
    #[case::other_unix_config(Platform::OtherUnix, BaseDir::Config, &[".config", "rudo"])]
    #[case::other_unix_data(Platform::OtherUnix, BaseDir::Data, &[".local", "share", "rudo"])]
    #[case::other_unix_state(Platform::OtherUnix, BaseDir::State, &[".local", "state", "rudo"])]
    #[case::windows_config(Platform::Windows, BaseDir::Config, &["AppData", "Roaming", "rudo"])]
    #[case::windows_data(Platform::Windows, BaseDir::Data, &["Documents", "RuDo"])]
    #[case::windows_state(Platform::Windows, BaseDir::State, &["AppData", "Local", "rudo"])]
    fn resolves_the_default_folder_below_home(
        #[case] platform: Platform,
        #[case] base_dir: BaseDir,
        #[case] expected_below_home: &[&str],
    ) {
        let home = home();
        let expected = below(&home, expected_below_home);

        let system_under_test = base_dir.resolve(platform, &home, environment(&[]));

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn absolute_xdg_variable_wins_on_every_platform() {
        let home = home();
        let xdg_dir = absolute_path(&["xdg"]);
        let app_data = absolute_path(&["appdata"]).into_os_string();
        let expected = xdg_dir.join("rudo");

        let system_under_test: Vec<(Platform, BaseDir, PathBuf)> = Platform::iter()
            .flat_map(|platform| BaseDir::iter().map(move |base_dir| (platform, base_dir)))
            .map(|(platform, base_dir)| {
                let variables = [
                    (base_dir.env_var(), xdg_dir.clone().into_os_string()),
                    ("APPDATA", app_data.clone()),
                    ("LOCALAPPDATA", app_data.clone()),
                ];
                let resolved = base_dir.resolve(platform, &home, environment(&variables));
                (platform, base_dir, resolved)
            })
            .collect();

        for (platform, base_dir, resolved) in system_under_test {
            assert_eq!(resolved, expected, "{platform:?} {base_dir:?}");
        }
    }

    #[rstest]
    #[case::empty("")]
    #[case::relative("relative/config")]
    #[case::current_dir("./data")]
    #[case::tilde("~/state")]
    fn unusable_xdg_variable_is_ignored_on_every_platform(#[case] xdg_value: &str) {
        let home = home();
        let expected: Vec<PathBuf> = Platform::iter()
            .flat_map(|platform| BaseDir::iter().map(move |base_dir| (platform, base_dir)))
            .map(|(platform, base_dir)| base_dir.resolve(platform, &home, environment(&[])))
            .collect();

        let system_under_test: Vec<PathBuf> = Platform::iter()
            .flat_map(|platform| BaseDir::iter().map(move |base_dir| (platform, base_dir)))
            .map(|(platform, base_dir)| {
                let variables = [(base_dir.env_var(), OsString::from(xdg_value))];
                base_dir.resolve(platform, &home, environment(&variables))
            })
            .collect();

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::config(BaseDir::Config, "APPDATA")]
    #[case::state(BaseDir::State, "LOCALAPPDATA")]
    fn windows_uses_its_app_data_variable(#[case] base_dir: BaseDir, #[case] variable: &str) {
        let home = home();
        let app_data = absolute_path(&["Profiles", "alice", "AppData"]);
        let variables = [(variable, app_data.clone().into_os_string())];
        let expected = app_data.join("rudo");

        let system_under_test = base_dir.resolve(Platform::Windows, &home, environment(&variables));

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::config_unset(BaseDir::Config, None, "Roaming")]
    #[case::config_empty(BaseDir::Config, Some(""), "Roaming")]
    #[case::config_relative(BaseDir::Config, Some("AppData"), "Roaming")]
    #[case::state_unset(BaseDir::State, None, "Local")]
    #[case::state_empty(BaseDir::State, Some(""), "Local")]
    #[case::state_relative(BaseDir::State, Some("AppData"), "Local")]
    fn windows_without_usable_app_data_falls_back_to_the_profile(
        #[case] base_dir: BaseDir,
        #[case] value: Option<&str>,
        #[case] expected_folder: &str,
    ) {
        let home = home();
        let variables: Vec<(&str, OsString)> = value
            .into_iter()
            .flat_map(|value| [("APPDATA", value.into()), ("LOCALAPPDATA", value.into())])
            .collect();
        let expected = below(&home, &["AppData", expected_folder, "rudo"]);

        let system_under_test = base_dir.resolve(Platform::Windows, &home, environment(&variables));

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn unix_platforms_ignore_the_windows_variables() {
        let home = home();
        let app_data = absolute_path(&["appdata"]).into_os_string();
        let variables = [("APPDATA", app_data.clone()), ("LOCALAPPDATA", app_data)];
        let expected = [
            below(&home, &[".config", "rudo"]),
            below(&home, &[".local", "state", "rudo"]),
        ];

        let system_under_test: Vec<[PathBuf; 2]> = Platform::iter()
            .filter(|platform| *platform != Platform::Windows)
            .map(|platform| {
                [BaseDir::Config, BaseDir::State]
                    .map(|base_dir| base_dir.resolve(platform, &home, environment(&variables)))
            })
            .collect();

        assert!(
            system_under_test
                .iter()
                .all(|resolved| *resolved == expected)
        );
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
    fn app_dirs_are_absolute() {
        let system_under_test: Vec<PathBuf> = BaseDir::iter()
            .map(|base_dir| base_dir.app_dir().unwrap())
            .collect();

        assert!(system_under_test.iter().all(|dir| dir.is_absolute()));
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

    #[rstest]
    #[case::empty(&[], "")]
    #[case::file(&["Groceries.md"], "Groceries.md")]
    #[case::nested(&["Home", "Garden", "Seeds.md"], "Home/Garden/Seeds.md")]
    #[case::hidden(&[".trash", "Home"], ".trash/Home")]
    #[case::multi_byte(&["Küche", "Einkäufe.md"], "Küche/Einkäufe.md")]
    fn portable_separates_components_with_slashes(
        #[case] components: &[&str],
        #[case] expected: &str,
    ) {
        let path: PathBuf = components.iter().collect();

        let system_under_test = portable(&path).to_string();

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn portable_keeps_the_root_of_an_absolute_path() {
        let path = absolute_path(&["srv", "todo"]);
        let expected = if cfg!(windows) {
            "C:/srv/todo"
        } else {
            "/srv/todo"
        };

        let system_under_test = portable(&path).to_string();

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::slashes("Home/Garden/Seeds.md", &["Home", "Garden", "Seeds.md"])]
    #[case::backslashes(r"Home\Garden\Seeds.md", &["Home", "Garden", "Seeds.md"])]
    #[case::mixed(r"Home\Garden/Seeds.md", &["Home", "Garden", "Seeds.md"])]
    #[case::trailing_separator("Home/", &["Home"])]
    #[case::doubled_separator("Home//Seeds.md", &["Home", "Seeds.md"])]
    #[case::file("Seeds.md", &["Seeds.md"])]
    #[case::empty("", &[])]
    fn parse_portable_accepts_both_separators(#[case] text: &str, #[case] components: &[&str]) {
        let expected: PathBuf = components.iter().collect();

        let system_under_test = parse_portable(text);

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn portable_round_trips_through_parse_portable() {
        let path: PathBuf = ["Home", "Garden", "Seeds.md"].iter().collect();
        let expected = path.clone();

        let system_under_test = parse_portable(&portable(&path).to_string());

        assert_eq!(system_under_test, expected);
    }
}
