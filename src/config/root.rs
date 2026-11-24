//! Choosing the workspace folder from `--root`, `workspace.root` and the default.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::paths::expand_tilde;

/// Errors that occur while preparing the workspace folder.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RootError {
    /// An explicitly chosen workspace folder does not exist.
    #[error("workspace folder {} does not exist", path.display())]
    NotFound {
        /// Folder that was chosen.
        path: PathBuf,
    },
    /// An explicitly chosen workspace folder is not a directory.
    #[error("workspace folder {} is not a directory", path.display())]
    NotADirectory {
        /// Path that was chosen.
        path: PathBuf,
    },
    /// An explicitly chosen workspace folder cannot be inspected.
    #[error("cannot access workspace folder {}", path.display())]
    Inaccessible {
        /// Folder that was chosen.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: io::Error,
    },
    /// The default workspace folder could not be created.
    #[error("cannot create workspace folder {}", path.display())]
    Create {
        /// Folder that could not be created.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: io::Error,
    },
}

/// The workspace folder and how it was chosen.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum WorkspaceRoot {
    /// Chosen with `--root` or `workspace.root`; it must already exist.
    Explicit(PathBuf),
    /// RuDo's data directory, used when no folder was chosen; it is created when missing.
    Default(PathBuf),
}

impl WorkspaceRoot {
    /// Chooses the workspace folder without touching the file system.
    ///
    /// `cli_root` (from `--root`, relative to the current directory, with a leading `~`
    /// replaced by `home`) wins over `configured_root` (`workspace.root`, already resolved by
    /// [`Config::load`](super::Config::load)). Without either, `default_dir` is used.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::path::{Path, PathBuf};
    ///
    /// use rudo::config::root::WorkspaceRoot;
    ///
    /// let root = WorkspaceRoot::select(None, None, Path::new("/data/rudo"), Path::new("/home/a"));
    /// assert_eq!(root, WorkspaceRoot::Default(PathBuf::from("/data/rudo")));
    /// ```
    #[must_use]
    pub fn select(
        cli_root: Option<&Path>,
        configured_root: Option<&Path>,
        default_dir: &Path,
        home: &Path,
    ) -> Self {
        match (cli_root, configured_root) {
            (Some(cli_root), _) => Self::Explicit(expand_tilde(cli_root, home).into_owned()),
            (None, Some(configured_root)) => Self::Explicit(configured_root.to_path_buf()),
            (None, None) => Self::Default(default_dir.to_path_buf()),
        }
    }

    /// Returns the workspace folder.
    #[must_use]
    pub fn path(&self) -> &Path {
        match self {
            Self::Explicit(path) | Self::Default(path) => path,
        }
    }

    /// Makes sure the workspace folder exists: an explicit folder is checked, the default
    /// folder is created with its parents when missing.
    ///
    /// # Errors
    ///
    /// For an explicit folder, returns [`RootError::NotFound`] if it does not exist,
    /// [`RootError::NotADirectory`] if it is not a directory and [`RootError::Inaccessible`]
    /// if it cannot be inspected. For the default folder, returns [`RootError::Create`] if it
    /// cannot be created.
    pub fn ensure_exists(&self) -> Result<(), RootError> {
        match self {
            Self::Explicit(path) => check_directory(path),
            Self::Default(path) => fs::create_dir_all(path).map_err(|source| RootError::Create {
                path: path.clone(),
                source,
            }),
        }
    }
}

fn check_directory(path: &Path) -> Result<(), RootError> {
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => Ok(()),
        Ok(_) => Err(RootError::NotADirectory {
            path: path.to_path_buf(),
        }),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Err(RootError::NotFound {
            path: path.to_path_buf(),
        }),
        Err(source) => Err(RootError::Inaccessible {
            path: path.to_path_buf(),
            source,
        }),
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use tempfile::TempDir;

    use super::*;

    const HOME: &str = "/home/alice";
    const DEFAULT_DIR: &str = "/data/rudo";

    #[rstest]
    #[case::nothing_chosen(None, None, WorkspaceRoot::Default(DEFAULT_DIR.into()))]
    #[case::configured(None, Some("/srv/todo"), WorkspaceRoot::Explicit("/srv/todo".into()))]
    #[case::cli(Some("/cli/todo"), None, WorkspaceRoot::Explicit("/cli/todo".into()))]
    #[case::cli_wins(
        Some("/cli/todo"),
        Some("/srv/todo"),
        WorkspaceRoot::Explicit("/cli/todo".into())
    )]
    #[case::cli_relative_to_current_dir(
        Some("todo"),
        Some("/srv/todo"),
        WorkspaceRoot::Explicit("todo".into())
    )]
    #[case::cli_tilde(Some("~/todo"), None, WorkspaceRoot::Explicit("/home/alice/todo".into()))]
    fn selects_root(
        #[case] cli_root: Option<&str>,
        #[case] configured_root: Option<&str>,
        #[case] expected: WorkspaceRoot,
    ) {
        let cli_root = cli_root.map(Path::new);
        let configured_root = configured_root.map(Path::new);
        let default_dir = Path::new(DEFAULT_DIR);
        let home = Path::new(HOME);

        let system_under_test = WorkspaceRoot::select(cli_root, configured_root, default_dir, home);

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::explicit(WorkspaceRoot::Explicit("/srv/todo".into()))]
    #[case::default(WorkspaceRoot::Default("/srv/todo".into()))]
    fn path_returns_the_folder(#[case] root: WorkspaceRoot) {
        let expected = Path::new("/srv/todo");

        let system_under_test = root.path();

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn existing_explicit_root_is_accepted() {
        let directory = TempDir::new().unwrap();
        let root = WorkspaceRoot::Explicit(directory.path().to_path_buf());

        let system_under_test = root.ensure_exists();

        assert!(system_under_test.is_ok());
    }

    #[test]
    fn missing_explicit_root_is_an_error() {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("missing");
        let root = WorkspaceRoot::Explicit(path.clone());

        let system_under_test = root.ensure_exists();

        assert!(matches!(
            system_under_test,
            Err(RootError::NotFound { path: missing }) if missing == path
        ));
        assert!(!path.exists());
    }

    #[test]
    fn explicit_root_that_is_a_file_is_an_error() {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("todo.md");
        fs::write(&path, "").unwrap();
        let root = WorkspaceRoot::Explicit(path.clone());

        let system_under_test = root.ensure_exists();

        assert!(matches!(
            system_under_test,
            Err(RootError::NotADirectory { path: file }) if file == path
        ));
    }

    #[test]
    fn inaccessible_explicit_root_is_an_error() {
        let directory = TempDir::new().unwrap();
        let file = directory.path().join("todo.md");
        fs::write(&file, "").unwrap();
        let path = file.join("lists");
        let root = WorkspaceRoot::Explicit(path.clone());

        let system_under_test = root.ensure_exists();

        assert!(matches!(
            system_under_test,
            Err(RootError::Inaccessible { path: failed, .. }) if failed == path
        ));
    }

    #[test]
    fn missing_default_root_is_created() {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("share/rudo");
        let root = WorkspaceRoot::Default(path.clone());

        let system_under_test = root.ensure_exists();

        assert!(system_under_test.is_ok());
        assert!(path.is_dir());
    }

    #[test]
    fn uncreatable_default_root_is_an_error() {
        let directory = TempDir::new().unwrap();
        let file = directory.path().join("share");
        fs::write(&file, "").unwrap();
        let path = file.join("rudo");
        let root = WorkspaceRoot::Default(path.clone());

        let system_under_test = root.ensure_exists();

        assert!(matches!(
            system_under_test,
            Err(RootError::Create { path: failed, .. }) if failed == path
        ));
    }
}
