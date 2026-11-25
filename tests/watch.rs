//! Watching a real workspace folder: changes made by another program are reported.

use std::fs;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use rudo::storage::watch::{FsChange, PathState, WorkspaceWatcher};
use tempfile::TempDir;

/// Long enough for the slowest machine to report a change; a passing test returns early.
const WATCH_TIMEOUT: Duration = Duration::from_secs(30);

#[test]
fn real_watcher_reports_a_list_written_by_another_program() {
    let root = TempDir::new().unwrap();
    let file = root.path().join("home/inbox.md");
    fs::create_dir(root.path().join("home")).unwrap();
    fs::write(&file, "- [ ] a\n").unwrap();
    let (sender, receiver) = mpsc::channel();
    let expected = FsChange::new("home/inbox.md", PathState::File);
    let watcher = WorkspaceWatcher::start(root.path(), move |changes| {
        // The test may already have finished and dropped the receiver.
        drop(sender.send(changes));
    })
    .unwrap();
    fs::write(&file, "- [x] a\n").unwrap();

    let system_under_test = {
        let deadline = Instant::now() + WATCH_TIMEOUT;
        let mut reported = false;
        while !reported && let Some(timeout) = deadline.checked_duration_since(Instant::now()) {
            let Ok(changes) = receiver.recv_timeout(timeout) else {
                break;
            };
            reported = changes.contains(&expected);
        }
        drop(watcher);
        reported
    };

    assert!(system_under_test);
}
