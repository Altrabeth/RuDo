//! Watching a real workspace folder: changes made by another program are reported.

use std::fs;
use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use rudo::storage::watch::{FsChange, PathState, WorkspaceWatcher};
use tempfile::TempDir;

/// Long enough for the slowest machine to report a change; a passing test returns early.
const WATCH_TIMEOUT: Duration = Duration::from_secs(30);

/// Watches `root`, runs `change` and returns whether a batch containing `expected` arrives
/// within [`WATCH_TIMEOUT`]. Only the content of the batches counts, not their number, because
/// `FSEvents` on macOS coalesces and delays events.
fn reports_change(root: &Path, change: impl FnOnce(), expected: &FsChange) -> bool {
    let (sender, receiver) = mpsc::channel();
    let watcher = WorkspaceWatcher::start(root, move |changes| {
        // The test may already have finished and dropped the receiver.
        drop(sender.send(changes));
    })
    .unwrap();
    change();
    let deadline = Instant::now() + WATCH_TIMEOUT;
    let mut reported = false;
    while !reported && let Some(timeout) = deadline.checked_duration_since(Instant::now()) {
        let Ok(changes) = receiver.recv_timeout(timeout) else {
            break;
        };
        reported = changes.contains(expected);
    }
    drop(watcher);
    reported
}

#[test]
fn real_watcher_reports_a_list_written_by_another_program() {
    let root = TempDir::new().unwrap();
    let file = root.path().join("home/inbox.md");
    fs::create_dir(root.path().join("home")).unwrap();
    fs::write(&file, "- [ ] a\n").unwrap();
    let expected = FsChange::new("home/inbox.md", PathState::File);

    let system_under_test = reports_change(
        root.path(),
        || fs::write(&file, "- [x] a\n").unwrap(),
        &expected,
    );

    assert!(system_under_test);
}

/// `FSEvents` reports canonical paths, and every temporary folder on macOS lies below the
/// symbolic link `/var`, so this is the case of a workspace below a linked folder.
#[cfg(target_os = "macos")]
#[test]
fn fsevents_reports_a_change_in_a_workspace_below_a_symbolic_link() {
    let root = TempDir::new().unwrap();
    let file = root.path().join("inbox.md");
    fs::write(&file, "- [ ] a\n").unwrap();
    let canonical_root = fs::canonicalize(root.path()).unwrap();
    let expected = FsChange::new("inbox.md", PathState::File);

    let system_under_test = reports_change(
        root.path(),
        || fs::write(&file, "- [x] a\n").unwrap(),
        &expected,
    );

    assert_ne!(canonical_root, root.path());
    assert!(system_under_test);
}
