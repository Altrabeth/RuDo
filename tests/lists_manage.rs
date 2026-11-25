//! Renaming lists and folders with `r` and deleting them into the bin with `d d` from the
//! lists pane.

#[expect(
    dead_code,
    reason = "each test crate uses only part of the shared harness"
)]
mod common;

use std::path::PathBuf;

use insta::assert_snapshot;
use rstest::rstest;
use rudo::app::confirm::ConfirmOverlay;
use rudo::app::lists::EntryKey;
use rudo::app::prompt::PromptOverlay;
use rudo::app::{Mode, StatusMessage};
use rudo::markdown::document::TaskId;

use crate::common::TestApp;

/// Rows in display order: `Home/` (`Chores`, `Groceries`), `Inbox`. `Home/Chores` is selected
/// and active at the start.
const WORKSPACE: &[(&str, &str)] = &[
    ("Inbox.md", "- [ ] Call mum\n"),
    ("Home/Chores.md", "- [ ] Vacuum\n- [ ] Dust\n"),
    (
        "Home/Groceries.md",
        "- [ ] Milk\n\t- [x] Lactose-free\n- [ ] Bread\n- [ ] Eggs\n- [x] Apples\n- [ ] Rice\n\
         - [ ] Tea\n",
    ),
];

/// The [`WORKSPACE`] with the empty folder `Work`, which is selected: rows `Home/` (`Chores`,
/// `Groceries`), `Work/`, `Inbox`.
fn workspace_with_empty_folder() -> TestApp {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press("G O").type_text("Work").press("enter");
    test_app
}

/// Returns whether a file or folder exists at `path`, relative to the workspace root.
fn exists(test_app: &TestApp, path: &str) -> bool {
    test_app.app().workspace().root().join(path).exists()
}

fn list_key(test_app: &TestApp, path: &str) -> EntryKey {
    EntryKey::List(test_app.list_path(path))
}

fn prompt_of(test_app: &TestApp) -> &PromptOverlay {
    match test_app.app().mode() {
        Mode::Prompt(prompt) => prompt,
        mode => panic!("no prompt is open, the mode is {mode:?}"),
    }
}

fn confirm_of(test_app: &TestApp) -> &ConfirmOverlay {
    match test_app.app().mode() {
        Mode::Confirm(confirm) => confirm,
        mode => panic!("no confirmation dialog is open, the mode is {mode:?}"),
    }
}

#[test]
fn rename_prompt_starts_with_the_name_and_the_cursor_at_the_end() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press("r").type_text(" list");

    let system_under_test = prompt_of(&test_app).prompt().input().value();

    assert_eq!(system_under_test, "Chores list");
}

#[test]
fn rename_prompt_shows_the_name_and_the_folder() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press("r");

    let system_under_test = test_app.render(80, 12);

    assert_snapshot!(system_under_test);
}

#[test]
fn renamed_list_keeps_its_selected_task() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app
        .press("2 j 1 r ctrl-u")
        .type_text("Tasks")
        .press("enter");
    let expected = second_task(&test_app);

    let system_under_test = test_app
        .app()
        .task_outline()
        .and_then(|outline| outline.selected().copied());

    assert_eq!(system_under_test, Some(expected));
}

/// Returns the id of the second task of the active list.
fn second_task(test_app: &TestApp) -> TaskId {
    test_app
        .app()
        .active_list()
        .expect("a list is open")
        .content()
        .as_ref()
        .expect("the list is loaded")
        .document()
        .tasks()
        .nth(1)
        .expect("the list has two tasks")
        .id()
}

#[test]
fn renamed_list_stays_selected_and_active() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press("r ctrl-u").type_text("Tasks").press("enter");
    let expected_key = list_key(&test_app, "Home/Tasks.md");
    let expected_active = test_app.list_path("Home/Tasks.md");

    let system_under_test = test_app.app();

    assert_eq!(test_app.read("Home/Tasks.md"), "- [ ] Vacuum\n- [ ] Dust\n");
    assert!(!exists(&test_app, "Home/Chores.md"));
    assert_eq!(
        system_under_test.lists().outline().selected(),
        Some(&expected_key)
    );
    assert_eq!(system_under_test.lists().active(), Some(&expected_active));
    assert!(matches!(system_under_test.mode(), Mode::Normal));
    assert_eq!(system_under_test.status_message(), None);
}

#[test]
fn renaming_another_list_keeps_the_active_list() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app
        .press("j r ctrl-u")
        .type_text("Food")
        .press("enter");
    let expected_key = list_key(&test_app, "Home/Food.md");
    let expected_active = test_app.list_path("Home/Chores.md");

    let system_under_test = test_app.app().lists();

    assert_eq!(system_under_test.outline().selected(), Some(&expected_key));
    assert_eq!(system_under_test.active(), Some(&expected_active));
}

#[test]
fn renamed_folder_moves_its_lists_and_the_active_list() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app
        .press("g g r ctrl-u")
        .type_text("House")
        .press("enter");
    let expected_key = EntryKey::Folder(PathBuf::from("House"));
    let expected_active = test_app.list_path("House/Chores.md");

    let system_under_test = test_app.app().lists();

    assert_eq!(system_under_test.outline().selected(), Some(&expected_key));
    assert_eq!(system_under_test.active(), Some(&expected_active));
    assert_eq!(test_app.read("House/Groceries.md").lines().count(), 7);
    assert!(!test_app.is_folder("Home"));
}

#[test]
fn renamed_folder_keeps_its_fold_and_moves_into_display_order() {
    let mut test_app = workspace_with_empty_folder();
    test_app
        .press("g g z a r ctrl-u")
        .type_text("Zoo")
        .press("enter");

    let system_under_test = test_app.render(80, 8);

    assert_snapshot!(system_under_test);
}

#[test]
fn rename_changing_only_the_case_renames_the_file() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app
        .press("r ctrl-u")
        .type_text("chores")
        .press("enter");
    let expected_key = list_key(&test_app, "Home/chores.md");

    let system_under_test = test_app.app().lists().outline().selected();

    assert_eq!(system_under_test, Some(&expected_key));
    assert_eq!(
        test_app.read("Home/chores.md"),
        "- [ ] Vacuum\n- [ ] Dust\n"
    );
}

#[test]
fn rename_to_an_existing_name_becomes_an_error_message() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app
        .press("r ctrl-u")
        .type_text("Groceries")
        .press("enter");
    let file = test_app.app().workspace().root().join("Home/Groceries.md");
    let expected = StatusMessage::error(format!("{} already exists", file.display()));
    let expected_key = list_key(&test_app, "Home/Chores.md");

    let system_under_test = test_app.app();

    assert_eq!(system_under_test.status_message(), Some(&expected));
    assert_eq!(
        system_under_test.lists().outline().selected(),
        Some(&expected_key)
    );
    assert_eq!(
        test_app.read("Home/Chores.md"),
        "- [ ] Vacuum\n- [ ] Dust\n"
    );
    assert_eq!(test_app.read("Home/Groceries.md").lines().count(), 7);
}

#[test]
fn rename_with_an_invalid_name_keeps_the_prompt_open_with_the_reason() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press("r ctrl-u").type_text("a/b").press("enter");

    let system_under_test = prompt_of(&test_app).prompt();

    assert_eq!(
        system_under_test.error(),
        Some("names must not contain '/'")
    );
    assert!(exists(&test_app, "Home/Chores.md"));
}

#[test]
fn cancelled_rename_changes_nothing() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press("r ctrl-u").type_text("Tasks").press("esc");
    let expected_key = list_key(&test_app, "Home/Chores.md");

    let system_under_test = test_app.app();

    assert!(matches!(system_under_test.mode(), Mode::Normal));
    assert_eq!(
        system_under_test.lists().outline().selected(),
        Some(&expected_key)
    );
    assert!(!exists(&test_app, "Home/Tasks.md"));
}

#[rstest]
#[case::rename("r")]
#[case::delete("d d")]
fn empty_workspace_ignores_rename_and_delete(#[case] keys: &str) {
    let mut test_app = TestApp::new(&[]);
    test_app.press(keys);

    let system_under_test = test_app.app();

    assert!(matches!(system_under_test.mode(), Mode::Normal));
    assert_eq!(system_under_test.status_message(), None);
}

#[rstest]
#[case::list_with_several_tasks("j", "Move list \"Groceries\" with 7 tasks to the bin?")]
#[case::list_with_one_task("G", "Move list \"Inbox\" with 1 task to the bin?")]
fn delete_list_asks_with_its_name_and_task_count(#[case] keys: &str, #[case] expected: &str) {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press(keys).press("d d");

    let system_under_test = confirm_of(&test_app);

    assert_eq!(system_under_test.message(), expected);
}

#[test]
fn delete_list_that_cannot_be_loaded_asks_without_a_task_count() {
    let mut test_app = TestApp::from_bytes(&[("Binary.md", &[0xff])]);
    test_app.press("d d");

    let system_under_test = confirm_of(&test_app);

    assert_eq!(
        system_under_test.message(),
        "Move list \"Binary\" to the bin?"
    );
}

#[test]
fn delete_dialog_shows_the_question_and_its_keys() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press("j d d");

    let system_under_test = test_app.render(80, 12);

    assert_snapshot!(system_under_test);
}

#[rstest]
#[case::first_entry_selects_the_next("", "y", "Home/Chores.md", "Home/Groceries.md")]
#[case::confirmed_with_enter("", "enter", "Home/Chores.md", "Home/Groceries.md")]
#[case::entry_in_the_middle_selects_the_next("j", "y", "Home/Groceries.md", "Inbox.md")]
#[case::last_entry_selects_the_previous("G", "y", "Inbox.md", "Home/Groceries.md")]
fn confirmed_delete_removes_the_list_and_selects_a_neighbour(
    #[case] keys: &str,
    #[case] confirm_key: &str,
    #[case] deleted: &str,
    #[case] expected_selection: &str,
) {
    let mut test_app = TestApp::new(WORKSPACE);
    let expected_key = list_key(&test_app, expected_selection);
    test_app.press(keys).press("d d").press(confirm_key);

    let system_under_test = test_app.app();

    assert!(!exists(&test_app, deleted));
    assert_eq!(
        system_under_test.lists().outline().selected(),
        Some(&expected_key)
    );
    assert!(matches!(system_under_test.mode(), Mode::Normal));
    assert_eq!(system_under_test.status_message(), None);
}

#[rstest]
#[case::no("n")]
#[case::escape("esc")]
fn declined_delete_keeps_the_list(#[case] key: &str) {
    let mut test_app = TestApp::new(WORKSPACE);
    let expected_key = list_key(&test_app, "Home/Chores.md");
    test_app.press("d d").press(key);

    let system_under_test = test_app.app();

    assert!(matches!(system_under_test.mode(), Mode::Normal));
    assert!(exists(&test_app, "Home/Chores.md"));
    assert_eq!(
        system_under_test.lists().outline().selected(),
        Some(&expected_key)
    );
}

#[test]
fn keys_of_the_lists_pane_do_nothing_in_the_dialog() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press("d d j q o");

    let system_under_test = test_app.app();

    assert!(matches!(system_under_test.mode(), Mode::Confirm(_)));
    assert!(!system_under_test.should_quit());
    assert!(exists(&test_app, "Home/Chores.md"));
}

#[test]
fn deleting_the_active_list_closes_it() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press("d d y");

    let system_under_test = test_app.app().lists().active();

    assert_eq!(system_under_test, None);
}

#[test]
fn deleting_another_list_keeps_the_active_list() {
    let mut test_app = TestApp::new(WORKSPACE);
    let expected = test_app.list_path("Home/Chores.md");
    test_app.press("G d d y");

    let system_under_test = test_app.app().lists().active();

    assert_eq!(system_under_test, Some(&expected));
}

#[test]
fn delete_empty_folder_asks_and_removes_it() {
    let mut test_app = workspace_with_empty_folder();
    let message = confirm_of(test_app.press("d d")).message().to_owned();
    test_app.press("y");
    let expected_key = list_key(&test_app, "Inbox.md");

    let system_under_test = test_app.app();

    assert_eq!(message, "Move folder \"Work\" to the bin?");
    assert!(!test_app.is_folder("Work"));
    assert_eq!(
        system_under_test.lists().outline().selected(),
        Some(&expected_key)
    );
}

#[test]
fn delete_non_empty_folder_asks_with_its_lists_and_tasks() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press("g g d d");
    let expected = "Move folder \"Home\" with 2 lists and 9 tasks to the bin?";

    let system_under_test = confirm_of(&test_app);

    assert_eq!(system_under_test.message(), expected);
}

#[test]
fn confirmed_delete_moves_a_non_empty_folder_to_the_bin() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press("g g d d y");
    let expected_key = list_key(&test_app, "Inbox.md");

    let system_under_test = test_app.app();

    assert!(!test_app.is_folder("Home"));
    assert_eq!(
        test_app.read(".trash/Home/Chores.md"),
        "- [ ] Vacuum\n- [ ] Dust\n"
    );
    assert_eq!(
        system_under_test.lists().outline().selected(),
        Some(&expected_key)
    );
}

#[test]
fn folder_with_only_hidden_files_moves_to_the_bin_with_them() {
    let mut test_app = TestApp::new(&[("Inbox.md", ""), ("Work/.hidden", "")]);
    test_app.press("g g d d y");

    let system_under_test = test_app.app();

    assert_eq!(system_under_test.status_message(), None);
    assert!(!test_app.is_folder("Work"));
    assert!(exists(&test_app, ".trash/Work/.hidden"));
}
