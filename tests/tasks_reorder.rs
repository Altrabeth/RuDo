//! Reordering and indenting tasks with `J`, `K`, `>` and `<`.

#[expect(
    dead_code,
    reason = "each test crate uses only part of the shared harness"
)]
mod common;

use rstest::rstest;
use rudo::markdown::document::TaskId;

use crate::common::TestApp;

/// The tasks in display order: `Buy milk` (`Oat milk`), `Bake bread`, `Buy candles`.
const GROCERIES: &str = "- [ ] Buy milk\n\t- [ ] Oat milk\n- [ ] Bake bread\n- [ ] Buy candles\n";

/// Returns the id of the task described as `description` in `Groceries.md`.
fn task_id(test_app: &TestApp, description: &str) -> TaskId {
    let list = test_app.list_path("Groceries.md");
    let document = test_app
        .app()
        .workspace()
        .list(&list)
        .unwrap()
        .content()
        .unwrap()
        .document();
    document
        .tasks()
        .find(|node| node.task().description() == description)
        .unwrap_or_else(|| panic!("Groceries.md has no task {description}"))
        .id()
}

/// Returns the selected task of the active list.
fn selected_task(test_app: &TestApp) -> Option<TaskId> {
    test_app
        .app()
        .task_outline()
        .and_then(|outline| outline.selected().copied())
}

#[rstest]
#[case::move_down_with_subtasks(
    "2 J",
    "- [ ] Bake bread\n- [ ] Buy milk\n\t- [ ] Oat milk\n- [ ] Buy candles\n"
)]
#[case::move_up_past_subtasks(
    "2 j j K",
    "- [ ] Bake bread\n- [ ] Buy milk\n\t- [ ] Oat milk\n- [ ] Buy candles\n"
)]
#[case::indent_after_the_last_subtask(
    "2 j j >",
    "- [ ] Buy milk\n\t- [ ] Oat milk\n\t- [ ] Bake bread\n- [ ] Buy candles\n"
)]
#[case::indent_under_a_task_without_subtasks(
    "2 G >",
    "- [ ] Buy milk\n\t- [ ] Oat milk\n- [ ] Bake bread\n\t- [ ] Buy candles\n"
)]
#[case::outdent(
    "2 j <",
    "- [ ] Buy milk\n- [ ] Oat milk\n- [ ] Bake bread\n- [ ] Buy candles\n"
)]
fn keys_restructure_the_list_and_save_it(#[case] keys: &str, #[case] expected: &str) {
    let mut test_app = TestApp::new(&[("Groceries.md", GROCERIES)]);

    let system_under_test = test_app.press(keys);

    assert_eq!(system_under_test.read("Groceries.md"), expected);
}

#[test]
fn indent_in_a_list_without_indentation_uses_the_configured_indent() {
    let mut test_app = TestApp::new(&[("Work.md", "- [ ] Write report\n- [ ] Send mail\n")]);
    let expected = "- [ ] Write report\n\t- [ ] Send mail\n";

    let system_under_test = test_app.press("2 j >");

    assert_eq!(system_under_test.read("Work.md"), expected);
}

#[rstest]
#[case::move_down("2 J", "Buy milk")]
#[case::move_down_then_up_a_row("2 J k", "Bake bread")]
#[case::move_up("2 G K", "Buy candles")]
#[case::move_up_then_down_a_row("2 G K j", "Bake bread")]
#[case::move_twice("2 J J k", "Buy candles")]
#[case::indent("2 G >", "Buy candles")]
#[case::outdent("2 j <", "Oat milk")]
fn selection_follows_the_moved_task(#[case] keys: &str, #[case] expected: &str) {
    let mut test_app = TestApp::new(&[("Groceries.md", GROCERIES)]);
    test_app.press(keys);
    let expected = task_id(&test_app, expected);

    let system_under_test = selected_task(&test_app);

    assert_eq!(system_under_test, Some(expected));
}

#[rstest]
#[case::move_up_the_first_task("2 K")]
#[case::move_down_the_last_task("2 G J")]
#[case::move_up_the_first_subtask("2 j K")]
#[case::move_down_the_last_subtask("2 j J")]
#[case::indent_the_first_task("2 >")]
#[case::indent_the_first_subtask("2 j >")]
#[case::outdent_a_top_level_task("2 <")]
#[case::keys_in_the_lists_pane("J K > <")]
fn boundaries_leave_the_file_untouched(#[case] keys: &str) {
    let mut test_app = TestApp::new(&[("Groceries.md", GROCERIES)]);

    let system_under_test = test_app.press(keys);

    assert_eq!(system_under_test.read("Groceries.md"), GROCERIES);
}

#[rstest]
#[case::selects_the_indented_task("2 h j >", "Bake bread")]
#[case::unfolds_the_new_parent("2 h j > k", "Oat milk")]
fn indent_under_a_folded_sibling_keeps_the_task_visible(
    #[case] keys: &str,
    #[case] expected: &str,
) {
    let mut test_app = TestApp::new(&[("Groceries.md", GROCERIES)]);
    test_app.press(keys);
    let expected = task_id(&test_app, expected);

    let system_under_test = selected_task(&test_app);

    assert_eq!(system_under_test, Some(expected));
}
