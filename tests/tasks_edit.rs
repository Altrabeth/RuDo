//! Editing the text of a task inline with `i` and `enter`.

#[expect(
    dead_code,
    reason = "each test crate uses only part of the shared harness"
)]
mod common;

use insta::assert_snapshot;
use rstest::rstest;
use rudo::app::{Mode, Severity};

use crate::common::TestApp;

/// A task with a tag and an inline field, followed by a second task.
const GROCERIES: &str =
    "# Groceries\n\n- [ ] Buy milk #shop [due:: 2026-12-01]\n- [ ] Bake bread\n";

/// Opens `Groceries.md` with `content` and focuses the tasks pane.
fn groceries(content: &str) -> TestApp {
    let mut test_app = TestApp::new(&[("Groceries.md", content)]);
    test_app.press("2");
    test_app
}

/// Returns the text and the cursor of the open inline editor.
fn inline_input(test_app: &TestApp) -> (String, usize) {
    match test_app.app().mode() {
        Mode::InlineEdit(edit) => (edit.input().value().to_owned(), edit.input().cursor()),
        mode => panic!("no inline editor is open, the mode is {mode:?}"),
    }
}

#[rstest]
#[case::i("i")]
#[case::enter("enter")]
fn edit_prefills_the_description_without_fields_and_the_cursor_at_the_end(#[case] key: &str) {
    let mut test_app = groceries(GROCERIES);
    test_app.press(key);
    let expected_text = "Buy milk #shop";
    let expected = (expected_text.to_owned(), expected_text.len());

    let system_under_test = inline_input(&test_app);

    assert_eq!(system_under_test, expected);
}

#[test]
fn submit_keeps_inline_fields_and_tags() {
    let mut test_app = groceries(GROCERIES);
    test_app.press("i").type_text(" now").press("enter");
    let expected = "# Groceries\n\n- [ ] Buy milk #shop now [due:: 2026-12-01]\n- [ ] Bake bread\n";

    let system_under_test = test_app.read("Groceries.md");

    assert_eq!(system_under_test, expected);
    assert!(matches!(test_app.app().mode(), Mode::Normal));
}

#[test]
fn unchanged_submit_leaves_the_file_byte_identical() {
    let content = "*   [ ]   Buy milk   [due::2026-12-01]\r\n- [ ] Bake bread";
    let mut test_app = groceries(content);
    test_app.press("i enter");

    let system_under_test = test_app.read("Groceries.md");

    assert_eq!(system_under_test, content);
}

#[test]
fn cancel_changes_nothing() {
    let mut test_app = groceries(GROCERIES);
    test_app.press("i").type_text(" now").press("esc");

    let system_under_test = test_app.read("Groceries.md");

    assert_eq!(system_under_test, GROCERIES);
    assert!(matches!(test_app.app().mode(), Mode::Normal));
}

#[test]
fn multi_byte_text_is_edited_by_grapheme() {
    let mut test_app = groceries("- [ ] Grüße 日本\n");
    test_app
        .press("i backspace backspace backspace backspace")
        .type_text("en aus 東京")
        .press("enter");
    let expected = "- [ ] Grüßen aus 東京\n";

    let system_under_test = test_app.read("Groceries.md");

    assert_eq!(system_under_test, expected);
}

#[test]
fn submit_after_the_task_disappeared_shows_an_error() {
    let mut test_app = groceries(GROCERIES);
    // Parsing assigns ids in order, so only the id of the second task disappears.
    let reloaded = "- [ ] Bake bread\n";
    let list = test_app.list_path("Groceries.md");
    test_app.press("j i").type_text(" now");
    test_app.write("Groceries.md", reloaded);
    test_app.run(|app| {
        app.reload_list(&list);
        Vec::new()
    });
    test_app.press("enter");

    let system_under_test = test_app.app().status_message().unwrap();

    assert_eq!(system_under_test.severity(), Severity::Error);
    assert_eq!(system_under_test.text(), "the edited task no longer exists");
    assert_eq!(test_app.read("Groceries.md"), reloaded);
    assert!(matches!(test_app.app().mode(), Mode::Normal));
}

#[test]
fn edited_row_shows_the_input_in_place_of_the_description() {
    let mut test_app = groceries(GROCERIES);
    test_app.press("i").type_text(" now");

    let system_under_test = test_app.render(80, 10);

    assert_snapshot!(system_under_test);
}
