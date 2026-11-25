//! Yanking and pasting tasks with their subtasks with `yy`, `p` and `P`, also between lists.

#[expect(
    dead_code,
    reason = "each test crate uses only part of the shared harness"
)]
mod common;

use std::collections::HashSet;

use rstest::rstest;
use rudo::app::StatusMessage;
use rudo::markdown::document::{TaskId, TodoDocument};

use crate::common::TestApp;

/// The tasks in display order: `Buy milk` (`Oat milk`), `Bake bread`.
const GROCERIES: &str = "- [ ] Buy milk\n\t- [ ] Oat milk\n- [ ] Bake bread\n";

/// The first list of the workspace, indented with tabs: `Trip` (`Pack`), `Stay`.
const INBOX: &str = "- [ ] Trip\n\t- [ ] Pack\n- [ ] Stay\n";

/// Returns the document of the active list.
fn active_document(test_app: &TestApp) -> &TodoDocument {
    test_app
        .app()
        .active_list()
        .unwrap()
        .content()
        .unwrap()
        .document()
}

/// Returns the selected task of the active list.
fn selected_task(test_app: &TestApp) -> Option<TaskId> {
    test_app
        .app()
        .task_outline()
        .and_then(|outline| outline.selected().copied())
}

#[rstest]
#[case::below_the_block_of_the_selected_task(
    "2 y y j j p",
    "- [ ] Buy milk\n\t- [ ] Oat milk\n- [ ] Bake bread\n- [ ] Buy milk\n\t- [ ] Oat milk\n"
)]
#[case::above_the_selected_task(
    "2 G y y g g P",
    "- [ ] Bake bread\n- [ ] Buy milk\n\t- [ ] Oat milk\n- [ ] Bake bread\n"
)]
#[case::as_a_sibling_of_a_subtask(
    "2 G y y k p",
    "- [ ] Buy milk\n\t- [ ] Oat milk\n\t- [ ] Bake bread\n- [ ] Bake bread\n"
)]
#[case::moved_with_delete("2 d d p", "- [ ] Bake bread\n- [ ] Buy milk\n\t- [ ] Oat milk\n")]
fn paste_copies_the_block_within_the_list(#[case] keys: &str, #[case] expected: &str) {
    let mut test_app = TestApp::new(&[("Groceries.md", GROCERIES)]);

    let system_under_test = test_app.press(keys);

    assert_eq!(system_under_test.read("Groceries.md"), expected);
}

#[rstest]
#[case::below_the_selected_task(
    "2 d d 1 j enter p",
    "- [ ] Summer\n  - [ ] Swim\n- [ ] Trip\n  - [ ] Pack\n"
)]
#[case::as_a_sibling_of_a_subtask(
    "2 d d 1 j enter j P",
    "- [ ] Summer\n  - [ ] Trip\n    - [ ] Pack\n  - [ ] Swim\n"
)]
fn delete_then_paste_moves_the_block_to_the_indentation_of_another_list(
    #[case] keys: &str,
    #[case] expected: &str,
) {
    let expected_source = "- [ ] Stay\n";
    let mut test_app = TestApp::new(&[
        ("Inbox.md", INBOX),
        ("Projects.md", "- [ ] Summer\n  - [ ] Swim\n"),
    ]);

    let system_under_test = test_app.press(keys);

    assert_eq!(system_under_test.read("Inbox.md"), expected_source);
    assert_eq!(system_under_test.read("Projects.md"), expected);
}

#[test]
fn pasted_lines_take_the_line_endings_of_the_target() {
    let expected = "- [ ] Summer\r\n  - [ ] Swim\r\n- [ ] Trip\r\n  - [ ] Pack\r\n";
    let mut test_app = TestApp::new(&[
        ("Inbox.md", INBOX),
        ("Projects.md", "- [ ] Summer\r\n  - [ ] Swim\r\n"),
    ]);

    let system_under_test = test_app.press("2 y y 1 j enter p");

    assert_eq!(system_under_test.read("Projects.md"), expected);
}

#[test]
fn paste_into_a_list_without_tasks_appends_the_block() {
    let expected = "# Projects\n- [ ] Trip\n\t- [ ] Pack\n";
    let mut test_app = TestApp::new(&[("Inbox.md", INBOX), ("Projects.md", "# Projects\n")]);

    let system_under_test = test_app.press("2 y y 1 j enter p");

    assert_eq!(system_under_test.read("Projects.md"), expected);
}

#[rstest]
#[case::block_of_two("2 y y", "yanked 2 tasks")]
#[case::single_task("2 G y y", "yanked 1 task")]
#[case::empty_register("2 p", "nothing to paste")]
fn yank_and_paste_report_in_the_status_bar(#[case] keys: &str, #[case] expected: &str) {
    let mut test_app = TestApp::new(&[("Groceries.md", GROCERIES)]);
    let expected = StatusMessage::info(expected);

    let system_under_test = test_app.press(keys);

    assert_eq!(system_under_test.app().status_message(), Some(&expected));
}

#[rstest]
#[case::empty_register("2 p P")]
#[case::yank("2 y y")]
#[case::paste_then_undo("2 y y p u")]
fn file_is_unchanged(#[case] keys: &str) {
    let mut test_app = TestApp::new(&[("Groceries.md", GROCERIES)]);

    let system_under_test = test_app.press(keys);

    assert_eq!(system_under_test.read("Groceries.md"), GROCERIES);
}

#[test]
fn pasted_root_is_selected() {
    let mut test_app = TestApp::new(&[("Groceries.md", GROCERIES)]);
    test_app.press("2 y y G p");
    let expected = active_document(&test_app)
        .tasks()
        .filter(|node| node.depth() == 0)
        .last()
        .map(|node| node.id());

    let system_under_test = selected_task(&test_app);

    assert_eq!(system_under_test, expected);
}

#[test]
fn pasting_twice_gives_distinct_tasks() {
    let expected_task_count = 7;
    let mut test_app = TestApp::new(&[("Groceries.md", GROCERIES)]);
    test_app.press("2 y y p p");

    let system_under_test: HashSet<TaskId> = active_document(&test_app)
        .tasks()
        .map(|node| node.id())
        .collect();

    assert_eq!(system_under_test.len(), expected_task_count);
}
