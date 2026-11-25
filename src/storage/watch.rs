//! Watching the workspace folder for changes made by other programs, such as Obsidian, an
//! editor or `git`.
//!
//! [`WorkspaceWatcher`] watches the folder recursively and debounces the events of the
//! operating system for [`DEBOUNCE_TIMEOUT`], so that a save, which editors often perform by
//! writing a temporary file and renaming it over the original, arrives as one batch. Every
//! batch becomes a list of [`FsChange`]s that say what is at each changed path *now*; which
//! of them matter is decided by [`App::on_fs_changed`](crate::app::App::on_fs_changed).

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use notify_debouncer_full::notify::{self, Event, RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{
    DebounceEventResult, DebouncedEvent, Debouncer, RecommendedCache, new_debouncer,
};
use thiserror::Error;

/// How long the watcher waits for further events on a path before it reports a batch.
pub const DEBOUNCE_TIMEOUT: Duration = Duration::from_millis(200);

/// What is at a changed path after the change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::EnumIter)]
pub enum PathState {
    /// A file, or a symbolic link to one.
    File,
    /// A directory.
    Directory,
    /// Nothing, or something that cannot be inspected.
    Missing,
}

/// A path of the workspace that changed, with what is there now.
///
/// The path is relative to the workspace root. An empty path stands for the workspace folder
/// itself, which the watcher also reports when the operating system may have dropped events.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FsChange {
    path: PathBuf,
    state: PathState,
}

impl FsChange {
    /// Creates the change of `path`, relative to the workspace root, now in `state`.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>, state: PathState) -> Self {
        Self {
            path: path.into(),
            state,
        }
    }

    /// Returns the changed path relative to the workspace root.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns what is at the path now.
    #[must_use]
    pub fn state(&self) -> PathState {
        self.state
    }
}

/// The workspace folder cannot be watched, so live reload is disabled.
#[derive(Debug, Error)]
#[error("cannot watch {}", path.display())]
pub struct WatchError {
    path: PathBuf,
    #[source]
    source: notify::Error,
}

/// Watches the workspace folder recursively for as long as it lives.
///
/// Dropping it stops watching and waits for the thread that reports the changes.
#[derive(Debug)]
pub struct WorkspaceWatcher {
    _debouncer: Debouncer<RecommendedWatcher, RecommendedCache>,
}

impl WorkspaceWatcher {
    /// Starts watching the folder `root` recursively and calls `on_changes` on a background
    /// thread with every debounced batch of changes, deduplicated and sorted by path.
    ///
    /// Errors the operating system reports while watching are logged; when it may have
    /// dropped events, the batch contains the workspace folder itself (an empty path).
    ///
    /// # Errors
    ///
    /// Returns a [`WatchError`] if the watcher cannot be created or `root` cannot be watched,
    /// for example because a large workspace exceeds the inotify watch limit on Linux.
    pub fn start(
        root: &Path,
        mut on_changes: impl FnMut(Vec<FsChange>) + Send + 'static,
    ) -> Result<Self, WatchError> {
        let watch_error = |source| WatchError {
            path: root.to_path_buf(),
            source,
        };
        let watched_root = root.to_path_buf();
        let mut debouncer = new_debouncer(
            DEBOUNCE_TIMEOUT,
            None,
            move |result: DebounceEventResult| match result {
                Ok(events) => {
                    let changes = changes(&watched_root, &events);
                    if !changes.is_empty() {
                        on_changes(changes);
                    }
                }
                Err(errors) => {
                    for error in errors {
                        tracing::warn!(%error, "file system watcher reported an error");
                    }
                }
            },
        )
        .map_err(watch_error)?;
        debouncer
            .watch(root, RecursiveMode::Recursive)
            .map_err(watch_error)?;
        tracing::info!(root = %root.display(), "watching the workspace");
        Ok(Self {
            _debouncer: debouncer,
        })
    }
}

/// Turns the `events` of one batch into the changes of their paths below `root`, sorted and
/// without duplicates, each with what is there now.
fn changes(root: &Path, events: &[DebouncedEvent]) -> Vec<FsChange> {
    let mut paths: Vec<PathBuf> = events
        .iter()
        .flat_map(|event| changed_paths(root, event))
        .collect();
    paths.sort_unstable();
    paths.dedup();
    paths
        .into_iter()
        .map(|path| {
            let state = path_state(&root.join(&path));
            FsChange { path, state }
        })
        .collect()
}

/// Returns the paths of `event` relative to `root`, plus the root itself (an empty path) if
/// the operating system may have dropped events.
fn changed_paths<'event>(
    root: &'event Path,
    event: &'event Event,
) -> impl Iterator<Item = PathBuf> + 'event {
    event
        .paths
        .iter()
        .filter_map(move |path| path.strip_prefix(root).ok().map(Path::to_path_buf))
        .chain(event.need_rescan().then(PathBuf::new))
}

/// Returns what is at `path` now, following symbolic links as the workspace scan does.
fn path_state(path: &Path) -> PathState {
    // A path that cannot be inspected is treated as missing: a known entry there causes a
    // rescan, which reports the problem the same way as at start-up.
    fs::metadata(path).map_or(PathState::Missing, |metadata| {
        if metadata.is_dir() {
            PathState::Directory
        } else {
            PathState::File
        }
    })
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use notify_debouncer_full::notify::EventKind;
    use notify_debouncer_full::notify::event::{CreateKind, Flag, RemoveKind};
    use tempfile::TempDir;

    use super::*;

    fn event(kind: EventKind, paths: &[&Path]) -> DebouncedEvent {
        let event = paths
            .iter()
            .fold(Event::new(kind), |event, path| event.add_path(path.into()));
        DebouncedEvent::new(event, Instant::now())
    }

    #[test]
    fn changes_are_relative_sorted_unique_and_inspected() {
        let root = TempDir::new().unwrap();
        fs::create_dir(root.path().join("home")).unwrap();
        fs::write(root.path().join("inbox.md"), "").unwrap();
        let events = [
            event(
                EventKind::Create(CreateKind::Any),
                &[&root.path().join("inbox.md")],
            ),
            event(
                EventKind::Remove(RemoveKind::Any),
                &[&root.path().join("gone.md"), &root.path().join("home")],
            ),
            event(
                EventKind::Create(CreateKind::Any),
                &[&root.path().join("inbox.md")],
            ),
        ];
        let expected = vec![
            FsChange::new("gone.md", PathState::Missing),
            FsChange::new("home", PathState::Directory),
            FsChange::new("inbox.md", PathState::File),
        ];

        let system_under_test = changes(root.path(), &events);

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn paths_outside_the_root_are_dropped() {
        let root = TempDir::new().unwrap();
        let other = TempDir::new().unwrap();
        let events = [event(
            EventKind::Create(CreateKind::Any),
            &[&other.path().join("inbox.md")],
        )];

        let system_under_test = changes(root.path(), &events);

        assert_eq!(system_under_test, Vec::new());
    }

    #[test]
    fn dropped_events_report_the_root_itself() {
        let root = TempDir::new().unwrap();
        let events = [DebouncedEvent::new(
            Event::new(EventKind::Other).set_flag(Flag::Rescan),
            Instant::now(),
        )];
        let expected = vec![FsChange::new("", PathState::Directory)];

        let system_under_test = changes(root.path(), &events);

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn watch_error_names_the_folder_and_the_cause() {
        let missing = TempDir::new().unwrap().path().join("missing");

        let system_under_test = WorkspaceWatcher::start(&missing, |_| {}).unwrap_err();

        assert_eq!(
            system_under_test.to_string(),
            format!("cannot watch {}", missing.display())
        );
        assert!(std::error::Error::source(&system_under_test).is_some());
    }
}
