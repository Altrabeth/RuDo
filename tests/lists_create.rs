//! Creating lists and folders from the lists pane with `o` and `O`.

#[expect(
    dead_code,
    reason = "each test crate uses only part of the shared harness"
)]
mod common;

use std::path::PathBuf;

use insta::assert_snapshot;
use rstest::rstest;
use rudo::app::lists::EntryKey;
use rudo::app::prompt::PromptOverlay;
use rudo::app::{Focus, Mode, StatusMessage};

use crate::common::TestApp;

/// Rows in display order: `Home/` (`Chores`), `Inbox`.
const WORKSPACE: &[(&str, &str)] = &[
    ("Inbox.md", "- [ ] Call mum\n"),
    ("Home/Chores.md", "- [ ] Vacuum\n"),
];

fn prompt_of(test_app: &TestApp) -> &PromptOverlay {
    match test_app.app().mode() {
        Mode::Prompt(prompt) => prompt,
        mode => panic!("no prompt is open, the mode is {mode:?}"),
    }
}

#[test]
fn new_list_creates_the_file_and_selects_it() {
    let mut test_app = TestApp::new(&[]);
    test_app.press("o").type_text("Groceries").press("enter");
    let expected_key = EntryKey::List(test_app.list_path("Groceries.md"));

    let system_under_test = test_app.app();

    assert_eq!(test_app.read("Groceries.md"), "");
    assert_eq!(
        system_under_test.lists().outline().selected(),
        Some(&expected_key)
    );
    assert_eq!(system_under_test.focus(), Focus::Lists);
    assert!(matches!(system_under_test.mode(), Mode::Normal));
    assert_eq!(system_under_test.status_message(), None);
}

#[rstest]
#[case::next_to_a_list_in_a_folder("", "Home/Groceries.md")]
#[case::next_to_a_list_at_the_top_level("G", "Groceries.md")]
#[case::inside_the_selected_folder("g g", "Home/Groceries.md")]
#[case::inside_a_folded_folder_which_unfolds("g g z a", "Home/Groceries.md")]
fn new_list_goes_into_the_folder_of_the_selection(#[case] keys: &str, #[case] expected: &str) {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app
        .press(keys)
        .press("o")
        .type_text("Groceries")
        .press("enter");
    let expected_key = EntryKey::List(test_app.list_path(expected));

    let system_under_test = test_app.app().lists().outline().selected();

    assert_eq!(system_under_test, Some(&expected_key));
    assert_eq!(test_app.read(expected), "");
}

#[test]
fn new_list_keeps_the_display_order() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press("G o").type_text("Archive").press("enter");

    let system_under_test = test_app.render(80, 8);

    assert_snapshot!(system_under_test);
}

#[test]
fn new_folder_creates_the_folder_and_selects_it() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press("G O").type_text("Work").press("enter");
    let expected_key = EntryKey::Folder(PathBuf::from("Work"));

    let system_under_test = test_app.app();

    assert!(test_app.is_folder("Work"));
    assert_eq!(
        system_under_test.lists().outline().selected(),
        Some(&expected_key)
    );
    assert_eq!(system_under_test.focus(), Focus::Lists);
}

#[test]
fn new_folder_inside_a_folder() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press("O").type_text("Garden").press("enter");
    let expected_key = EntryKey::Folder(PathBuf::from("Home/Garden"));

    let system_under_test = test_app.app().lists().outline().selected();

    assert!(test_app.is_folder("Home/Garden"));
    assert_eq!(system_under_test, Some(&expected_key));
}

#[rstest]
#[case::empty("", "the name is empty")]
#[case::hidden(".hidden", "names must not start with a dot")]
#[case::path_separator("a/b", "names must not contain '/'")]
fn invalid_name_keeps_the_prompt_open_with_the_reason(
    #[case] name: &str,
    #[case] expected_error: &str,
) {
    let mut test_app = TestApp::new(&[]);
    test_app.press("o").type_text(name).press("enter");

    let system_under_test = prompt_of(&test_app);

    assert_eq!(system_under_test.prompt().error(), Some(expected_error));
    assert_eq!(system_under_test.prompt().input().value(), name);
    assert!(test_app.app().workspace().entries().is_empty());
}

#[test]
fn invalid_name_is_shown_in_the_prompt() {
    let mut test_app = TestApp::new(&[]);
    test_app.press("o").type_text("a/b").press("enter");

    let system_under_test = test_app.render(80, 12);

    assert_snapshot!(system_under_test);
}

#[test]
fn typing_after_an_invalid_name_clears_the_reason() {
    let mut test_app = TestApp::new(&[]);
    test_app
        .press("o")
        .type_text("a/b")
        .press("enter")
        .press("backspace");

    let system_under_test = prompt_of(&test_app);

    assert_eq!(system_under_test.prompt().error(), None);
}

#[test]
fn corrected_name_creates_the_list() {
    let mut test_app = TestApp::new(&[]);
    test_app
        .press("o")
        .type_text("a/b")
        .press("enter")
        .press("left backspace enter");

    let system_under_test = test_app.read("ab.md");

    assert_eq!(system_under_test, "");
}

#[rstest]
#[case::new_list("o")]
#[case::new_folder("O")]
fn cancel_closes_the_prompt_without_changes(#[case] key: &str) {
    let mut test_app = TestApp::new(&[]);
    test_app.press(key).type_text("Groceries").press("esc");

    let system_under_test = test_app.app();

    assert!(matches!(system_under_test.mode(), Mode::Normal));
    assert!(system_under_test.workspace().entries().is_empty());
    assert!(!test_app.is_folder("Groceries"));
}

#[test]
fn keys_of_the_lists_pane_are_typed_into_the_prompt() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press("o").type_text("jkq?");

    let system_under_test = prompt_of(&test_app);

    assert_eq!(system_under_test.prompt().input().value(), "jkq?");
    assert!(!test_app.app().should_quit());
}

#[test]
fn pasted_text_goes_into_the_prompt() {
    let mut test_app = TestApp::new(&[]);
    test_app
        .press("o")
        .run(|app| app.handle_paste("Groceries"))
        .press("enter");

    let system_under_test = test_app.read("Groceries.md");

    assert_eq!(system_under_test, "");
}

#[test]
fn name_taken_in_the_meantime_becomes_an_error_message() {
    let mut test_app = TestApp::new(&[]);
    test_app.press("o").type_text("Groceries");
    test_app.write("Groceries.md", "- [ ] Buy milk\n");
    test_app.press("enter");
    let expected = StatusMessage::error("Groceries.md already exists");

    let system_under_test = test_app.app();

    assert_eq!(system_under_test.status_message(), Some(&expected));
    assert!(matches!(system_under_test.mode(), Mode::Normal));
    assert_eq!(test_app.read("Groceries.md"), "- [ ] Buy milk\n");
}

#[test]
fn open_prompt_shows_the_target_folder_and_its_keys() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press("o").type_text("Groceries");

    let system_under_test = test_app.render(80, 12);

    assert_snapshot!(system_under_test);
}
