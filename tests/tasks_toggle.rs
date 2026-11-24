//! Toggling the selected task done and open again with `x` and `space`.

#[expect(
    dead_code,
    reason = "each test crate uses only part of the shared harness"
)]
mod common;

use std::path::Path;

use rstest::rstest;
use rudo::config::Config;

use crate::common::TestApp;

#[rstest]
#[case::x("x")]
#[case::space("space")]
fn open_task_becomes_done_with_todays_completion_date(#[case] key: &str) {
    let files = [("Groceries.md", "- [ ] Buy milk\n")];
    let expected = "- [x] Buy milk [completion:: 2026-03-14]\n";
    let mut test_app = TestApp::new(&files);

    let system_under_test = test_app.press("tab").press(key);

    assert_eq!(system_under_test.read("Groceries.md"), expected);
}

#[test]
fn done_task_becomes_open_without_its_completion_date() {
    let files = [("Groceries.md", "- [x] Buy milk [completion:: 2026-03-01]\n")];
    let expected = "- [ ] Buy milk\n";
    let mut test_app = TestApp::new(&files);

    let system_under_test = test_app.press("tab x");

    assert_eq!(system_under_test.read("Groceries.md"), expected);
}

#[test]
fn toggling_twice_restores_the_open_task() {
    let files = [("Groceries.md", "- [ ] Buy milk #shopping\n")];
    let expected = "- [ ] Buy milk #shopping\n";
    let mut test_app = TestApp::new(&files);

    let system_under_test = test_app.press("tab x x");

    assert_eq!(system_under_test.read("Groceries.md"), expected);
}

#[test]
fn without_record_completion_date_no_date_is_written() {
    let files = [("Groceries.md", "- [ ] Buy milk\n")];
    let config = Config::from_yaml(
        "tasks:\n  record_completion_date: false\n",
        Path::new("config.yaml"),
    )
    .unwrap();
    let expected = "- [x] Buy milk\n";
    let mut test_app = TestApp::with_config(&files, config);

    let system_under_test = test_app.press("tab x");

    assert_eq!(system_under_test.read("Groceries.md"), expected);
}

#[rstest]
#[case::in_progress("- [/] Buy milk\n")]
#[case::cancelled("- [-] Buy milk\n")]
fn other_statuses_become_done(#[case] content: &str) {
    let files = [("Groceries.md", content)];
    let expected = "- [x] Buy milk [completion:: 2026-03-14]\n";
    let mut test_app = TestApp::new(&files);

    let system_under_test = test_app.press("tab x");

    assert_eq!(system_under_test.read("Groceries.md"), expected);
}

#[test]
fn parent_is_toggled_without_its_subtasks() {
    let files = [("Groceries.md", "- [ ] Buy milk\n\t- [ ] Oat milk\n")];
    let expected = "- [x] Buy milk [completion:: 2026-03-14]\n\t- [ ] Oat milk\n";
    let mut test_app = TestApp::new(&files);

    let system_under_test = test_app.press("tab x");

    assert_eq!(system_under_test.read("Groceries.md"), expected);
}

#[test]
fn subtask_is_toggled_without_its_parent() {
    let files = [("Groceries.md", "- [ ] Buy milk\n\t- [ ] Oat milk\n")];
    let expected = "- [ ] Buy milk\n\t- [x] Oat milk [completion:: 2026-03-14]\n";
    let mut test_app = TestApp::new(&files);

    let system_under_test = test_app.press("tab j x");

    assert_eq!(system_under_test.read("Groceries.md"), expected);
}

#[test]
fn list_without_tasks_is_not_changed() {
    let files = [("Notes.md", "# Notes\n\nNothing to do.\n")];
    let expected = "# Notes\n\nNothing to do.\n";
    let mut test_app = TestApp::new(&files);

    let system_under_test = test_app.press("tab x");

    assert_eq!(system_under_test.read("Notes.md"), expected);
}

#[test]
fn toggled_task_is_shown_done_in_the_tasks_pane() {
    let files = [("Groceries.md", "- [ ] Buy milk\n")];
    let mut test_app = TestApp::new(&files);

    let system_under_test = test_app.press("tab x").render(80, 5).to_string();

    assert!(system_under_test.contains("[x] Buy milk"));
}
