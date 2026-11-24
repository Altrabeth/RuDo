//! Adding tasks inline with `o`, `O` and `a`.

#[expect(
    dead_code,
    reason = "each test crate uses only part of the shared harness"
)]
mod common;

use insta::assert_snapshot;
use rstest::rstest;
use rudo::app::Mode;
use rudo::markdown::document::TaskId;

use crate::common::TestApp;

/// Two top-level tasks; `Buy milk` has the subtask `Oat milk`.
const GROCERIES: &str = "# Groceries\n\n- [ ] Buy milk\n\t- [ ] Oat milk\n- [ ] Bake bread\n";

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

/// Opens `Groceries.md` with [`GROCERIES`] and focuses the tasks pane.
fn groceries() -> TestApp {
    let mut test_app = TestApp::new(&[("Groceries.md", GROCERIES)]);
    test_app.press("2");
    test_app
}

#[rstest]
#[case::below_the_whole_block(
    "o",
    "# Groceries\n\n- [ ] Buy milk\n\t- [ ] Oat milk\n- [ ] Eggs\n- [ ] Bake bread\n"
)]
#[case::below_a_subtask(
    "j o",
    "# Groceries\n\n- [ ] Buy milk\n\t- [ ] Oat milk\n\t- [ ] Eggs\n- [ ] Bake bread\n"
)]
#[case::above(
    "G O",
    "# Groceries\n\n- [ ] Buy milk\n\t- [ ] Oat milk\n- [ ] Eggs\n- [ ] Bake bread\n"
)]
#[case::subtask_after_the_last_subtask(
    "a",
    "# Groceries\n\n- [ ] Buy milk\n\t- [ ] Oat milk\n\t- [ ] Eggs\n- [ ] Bake bread\n"
)]
#[case::first_subtask_indented_by_a_tab(
    "G a",
    "# Groceries\n\n- [ ] Buy milk\n\t- [ ] Oat milk\n- [ ] Bake bread\n\t- [ ] Eggs\n"
)]
fn submit_inserts_the_typed_task_and_saves_it(#[case] keys: &str, #[case] expected: &str) {
    let mut test_app = groceries();
    test_app.press(keys).type_text("Eggs").press("enter");

    let system_under_test = test_app.read("Groceries.md");

    assert_eq!(system_under_test, expected);
}

#[rstest]
#[case::below("o")]
#[case::above("O")]
#[case::subtask("a")]
fn new_task_is_selected_and_the_mode_returns_to_normal(#[case] key: &str) {
    let mut test_app = groceries();
    test_app.press(key).type_text("Eggs").press("enter");
    let expected = task_id(&test_app, "Eggs");

    let system_under_test = selected_task(&test_app);

    assert_eq!(system_under_test, Some(expected));
    assert!(matches!(test_app.app().mode(), Mode::Normal));
}

#[rstest]
#[case::cancel("o", "Eggs", "esc")]
#[case::empty_text("o", "", "enter")]
#[case::only_blanks("a", "   ", "enter")]
fn cancel_or_empty_text_leaves_the_file_untouched(
    #[case] key: &str,
    #[case] text: &str,
    #[case] finish: &str,
) {
    let mut test_app = groceries();
    test_app.press(key).type_text(text).press(finish);
    let expected_selection = task_id(&test_app, "Buy milk");

    let system_under_test = test_app.read("Groceries.md");

    assert_eq!(system_under_test, GROCERIES);
    assert_eq!(selected_task(&test_app), Some(expected_selection));
    assert!(matches!(test_app.app().mode(), Mode::Normal));
}

#[test]
fn document_is_not_changed_while_typing() {
    let mut test_app = groceries();
    test_app.press("o").type_text("Eggs");

    let system_under_test = test_app.read("Groceries.md");

    assert_eq!(system_under_test, GROCERIES);
    assert!(matches!(test_app.app().mode(), Mode::InlineEdit(_)));
}

#[rstest]
#[case::empty_file_below("", "o", "- [ ] Eggs\n")]
#[case::empty_file_above("", "O", "- [ ] Eggs\n")]
#[case::empty_file_subtask("", "a", "- [ ] Eggs\n")]
#[case::only_prose(
    "# Notes\n\nNothing yet.\n",
    "a",
    "# Notes\n\nNothing yet.\n- [ ] Eggs\n"
)]
fn list_without_tasks_gets_a_top_level_task_at_the_end(
    #[case] content: &str,
    #[case] key: &str,
    #[case] expected: &str,
) {
    let mut test_app = TestApp::new(&[("Groceries.md", content)]);
    test_app
        .press("2")
        .press(key)
        .type_text("Eggs")
        .press("enter");

    let system_under_test = test_app.read("Groceries.md");

    assert_eq!(system_under_test, expected);
    assert_eq!(selected_task(&test_app), Some(task_id(&test_app, "Eggs")));
}

#[test]
fn typed_inline_fields_become_fields() {
    let mut test_app = groceries();
    let expected_file = "# Groceries\n\n- [ ] Buy milk\n\t- [ ] Oat milk\n- [ ] Eggs [due:: 2026-12-01]\n\
         - [ ] Bake bread\n";
    test_app
        .press("o")
        .type_text("Eggs [due:: 2026-12-01]")
        .press("enter");

    let system_under_test = test_app.read("Groceries.md");

    assert_eq!(system_under_test, expected_file);
    assert_eq!(selected_task(&test_app), Some(task_id(&test_app, "Eggs")));
}

#[test]
fn keys_bound_in_the_panes_are_typed_as_text() {
    let mut test_app = groceries();
    test_app
        .press("o")
        .type_text("quiz? 2 jokes")
        .press("enter");

    let system_under_test = test_app.read("Groceries.md");

    assert!(system_under_test.contains("- [ ] quiz? 2 jokes\n"));
    assert!(!test_app.app().should_quit());
}

#[test]
fn pasted_text_goes_into_the_input() {
    let mut test_app = groceries();
    test_app
        .press("o")
        .run(|app| app.handle_paste("Eggs\nq"))
        .press("enter");

    let system_under_test = test_app.read("Groceries.md");

    assert!(system_under_test.contains("- [ ] Eggs q\n"));
}

#[test]
fn new_subtask_on_a_folded_task_unfolds_it_first() {
    let mut test_app = groceries();
    test_app.press("h a esc j");
    let expected = task_id(&test_app, "Oat milk");

    let system_under_test = selected_task(&test_app);

    assert_eq!(system_under_test, Some(expected));
}

#[rstest]
#[case::below_a_folded_task("h o", "placeholder_below_a_folded_task")]
#[case::above("G O", "placeholder_above")]
#[case::subtask("a", "placeholder_subtask")]
#[case::in_an_empty_list("1 j enter 2 o", "placeholder_in_an_empty_list")]
fn placeholder_row_shows_the_input_at_the_insertion_point(
    #[case] keys: &str,
    #[case] snapshot: &str,
) {
    let mut test_app = TestApp::new(&[("Groceries.md", GROCERIES), ("Notes.md", "# Notes\n")]);
    test_app.press("2").press(keys).type_text("Eggs");

    let system_under_test = test_app.render(80, 10);

    assert_snapshot!(snapshot, system_under_test);
}
