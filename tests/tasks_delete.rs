//! Deleting the selected task with its subtasks with `dd`.

#[expect(
    dead_code,
    reason = "each test crate uses only part of the shared harness"
)]
mod common;

use std::path::Path;

use rstest::rstest;
use rudo::app::StatusMessage;
use rudo::config::Config;

use crate::common::TestApp;

/// The tasks in display order: `Buy milk` (`Oat milk` with a note, `Soy milk`), `Bake bread`,
/// `Buy candles`, followed by a paragraph after a blank line.
const GROCERIES: &str = concat!(
    "# Groceries\n",
    "- [ ] Buy milk\n",
    "\t- [ ] Oat milk\n",
    "\t  barista edition\n",
    "\n",
    "\t- [x] Soy milk\n",
    "- [ ] Bake bread\n",
    "- [ ] Buy candles\n",
    "\n",
    "Shop on Saturday.\n",
);

/// Returns the description of the selected task of the active list.
fn selected_description(test_app: &TestApp) -> Option<String> {
    let selected = *test_app.app().task_outline()?.selected()?;
    let document = test_app.app().active_list()?.content().ok()?.document();
    document
        .tasks()
        .find(|node| node.id() == selected)
        .map(|node| node.task().description().to_owned())
}

#[rstest]
#[case::with_subtasks_continuation_and_inner_blank(
    "2 d d",
    "# Groceries\n- [ ] Bake bread\n- [ ] Buy candles\n\nShop on Saturday.\n"
)]
#[case::subtask_with_continuation(
    "2 j d d",
    concat!(
        "# Groceries\n- [ ] Buy milk\n\n\t- [x] Soy milk\n- [ ] Bake bread\n",
        "- [ ] Buy candles\n\nShop on Saturday.\n",
    )
)]
#[case::last_task_keeps_the_trailing_blank_line(
    "2 G d d",
    concat!(
        "# Groceries\n- [ ] Buy milk\n\t- [ ] Oat milk\n\t  barista edition\n\n",
        "\t- [x] Soy milk\n- [ ] Bake bread\n\nShop on Saturday.\n",
    )
)]
fn delete_removes_the_block_and_saves_the_list(#[case] keys: &str, #[case] expected: &str) {
    let mut test_app = TestApp::new(&[("Groceries.md", GROCERIES)]);

    let system_under_test = test_app.press(keys);

    assert_eq!(system_under_test.read("Groceries.md"), expected);
}

#[rstest]
#[case::next_sibling("2 d d", Some("Bake bread"))]
#[case::next_sibling_of_a_subtask("2 j d d", Some("Soy milk"))]
#[case::previous_sibling("2 G d d", Some("Bake bread"))]
#[case::previous_sibling_of_a_subtask("2 j j d d", Some("Oat milk"))]
#[case::parent("2 j d d d d", Some("Buy milk"))]
#[case::nothing("2 d d d d d d", None)]
#[case::parent_when_the_next_sibling_is_hidden("2 H j d d", Some("Buy milk"))]
fn selection_moves_to_a_neighbour(#[case] keys: &str, #[case] expected: Option<&str>) {
    let mut test_app = TestApp::new(&[("Groceries.md", GROCERIES)]);
    test_app.press(keys);

    let system_under_test = selected_description(&test_app);

    assert_eq!(system_under_test.as_deref(), expected);
}

#[rstest]
#[case::block_of_three("2 d d", "deleted 3 tasks (u to undo)")]
#[case::single_task("2 G d d", "deleted 1 task (u to undo)")]
fn delete_reports_the_number_of_tasks(#[case] keys: &str, #[case] expected: &str) {
    let mut test_app = TestApp::new(&[("Groceries.md", GROCERIES)]);
    let expected = StatusMessage::info(expected);

    let system_under_test = test_app.press(keys);

    assert_eq!(system_under_test.app().status_message(), Some(&expected));
}

#[rstest]
#[case::rebound("keybindings:\n  undo: [ctrl-z]\n", "deleted 1 task (ctrl-z to undo)")]
#[case::unbound("keybindings:\n  undo: []\n", "deleted 1 task")]
fn delete_message_names_the_bound_undo_key(#[case] yaml: &str, #[case] expected: &str) {
    let config = Config::from_yaml(yaml, Path::new("config.yaml")).unwrap();
    let mut test_app = TestApp::with_config(&[("Groceries.md", GROCERIES)], config);
    let expected = StatusMessage::info(expected);

    let system_under_test = test_app.press("2 G d d");

    assert_eq!(system_under_test.app().status_message(), Some(&expected));
}

#[rstest]
#[case::task_with_subtasks("2 d d u")]
#[case::subtask("2 j d d u")]
#[case::last_task("2 G d d u")]
#[case::every_task("2 d d d d d d u u u")]
fn undo_restores_the_file_byte_for_byte(#[case] keys: &str) {
    let mut test_app = TestApp::new(&[("Groceries.md", GROCERIES)]);

    let system_under_test = test_app.press(keys);

    assert_eq!(system_under_test.read("Groceries.md"), GROCERIES);
}

#[test]
fn undo_selects_the_deleted_task_again() {
    let expected = Some("Bake bread");
    let mut test_app = TestApp::new(&[("Groceries.md", GROCERIES)]);
    test_app.press("2 j j j d d u");

    let system_under_test = selected_description(&test_app);

    assert_eq!(system_under_test.as_deref(), expected);
}

#[rstest]
#[case::empty_list("2 d d", "# Groceries\n")]
#[case::lists_pane_asks_first("d d", GROCERIES)]
fn delete_without_a_selected_task_leaves_the_file_untouched(
    #[case] keys: &str,
    #[case] expected: &str,
) {
    let mut test_app = TestApp::new(&[("Groceries.md", expected)]);

    let system_under_test = test_app.press(keys);

    assert_eq!(system_under_test.read("Groceries.md"), expected);
}
