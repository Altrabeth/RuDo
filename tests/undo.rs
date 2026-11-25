//! Undoing and redoing task edits with `u` and `ctrl-r`.

#[expect(
    dead_code,
    reason = "each test crate uses only part of the shared harness"
)]
mod common;

use rudo::app::StatusMessage;

use crate::common::TestApp;

/// Front matter, prose and odd spacing that every undo must restore byte for byte.
const ORIGINAL: &str = "---\ntags: [home]\n---\n# Groceries\n\n- [ ] Buy  milk\n* [ ] Eggs\n";

#[test]
fn undoing_add_toggle_and_indent_restores_the_original_bytes() {
    let files = [("Groceries.md", ORIGINAL)];
    let mut test_app = TestApp::new(&files);
    test_app
        .press("tab o")
        .type_text("Bread")
        .press("enter x > g g");

    let system_under_test = test_app.press("u u u");

    assert_eq!(system_under_test.read("Groceries.md"), ORIGINAL);
}

#[test]
fn redo_reapplies_the_undone_changes() {
    let files = [("Groceries.md", ORIGINAL)];
    let mut test_app = TestApp::new(&files);
    test_app.press("tab o").type_text("Bread").press("enter x");
    let expected = test_app.read("Groceries.md");
    test_app.press("u u");

    let system_under_test = test_app.press("ctrl-r ctrl-r");

    assert_eq!(system_under_test.read("Groceries.md"), expected);
}

#[test]
fn undo_restores_the_selection_and_names_the_list() {
    let files = [("Groceries.md", "- [ ] a\n- [ ] b\n")];
    let expected_message = StatusMessage::info("undo: Groceries");
    let mut test_app = TestApp::new(&files);
    test_app.press("tab j x k");

    let system_under_test = test_app.press("u");

    let outline = system_under_test.app().task_outline().unwrap();
    let selected = *outline.selected().unwrap();
    let loaded = system_under_test
        .app()
        .active_list()
        .unwrap()
        .content()
        .unwrap();
    let description = loaded
        .document()
        .tasks()
        .find(|node| node.id() == selected)
        .unwrap()
        .task()
        .description()
        .to_owned();
    assert_eq!(description, "b");
    assert_eq!(system_under_test.read("Groceries.md"), "- [ ] a\n- [ ] b\n");
    assert_eq!(
        system_under_test.app().status_message(),
        Some(&expected_message)
    );
}

#[test]
fn undo_switches_to_the_list_of_the_change() {
    let files = [("A.md", "- [ ] a\n"), ("B.md", "- [ ] b\n")];
    let mut test_app = TestApp::new(&files);
    let expected_list = test_app.list_path("A.md");
    test_app.press("tab x shift-tab j enter");

    let system_under_test = test_app.press("u");

    assert_eq!(system_under_test.read("A.md"), "- [ ] a\n");
    assert_eq!(
        system_under_test.app().lists().active(),
        Some(&expected_list)
    );
}

#[test]
fn a_new_edit_clears_the_redo_stack() {
    let files = [("Groceries.md", "- [ ] a\n- [ ] b\n")];
    let expected = "- [ ] a\n- [x] b [completion:: 2026-03-14]\n";
    let mut test_app = TestApp::new(&files);
    test_app.press("tab x u j x");

    let system_under_test = test_app.press("ctrl-r");

    assert_eq!(system_under_test.read("Groceries.md"), expected);
}

#[test]
fn undo_without_history_changes_nothing() {
    let files = [("Groceries.md", "- [ ] a\n")];
    let expected_message = StatusMessage::info("nothing to undo");
    let mut test_app = TestApp::new(&files);

    let system_under_test = test_app.press("tab u");

    assert_eq!(system_under_test.read("Groceries.md"), "- [ ] a\n");
    assert_eq!(
        system_under_test.app().status_message(),
        Some(&expected_message)
    );
}

#[test]
fn history_remembers_the_last_hundred_changes() {
    let files = [("Groceries.md", "- [ ] a\n")];
    let mut test_app = TestApp::new(&files);
    test_app.press("tab x");
    let expected = test_app.read("Groceries.md");
    test_app.press(&"x ".repeat(100));

    let system_under_test = test_app.press(&"u ".repeat(101));

    assert_eq!(system_under_test.read("Groceries.md"), expected);
}

#[test]
fn external_change_forgets_the_history_of_the_list() {
    let files = [("Groceries.md", "- [ ] a\n")];
    let external = "- [ ] changed elsewhere\n";
    let mut test_app = TestApp::new(&files);
    test_app.press("tab x");
    test_app.write("Groceries.md", external);
    let list = test_app.list_path("Groceries.md");
    test_app.run(|app| {
        app.reload_list(&list);
        Vec::new()
    });

    let system_under_test = test_app.press("u");

    assert_eq!(system_under_test.read("Groceries.md"), external);
}

#[test]
fn failed_save_of_a_changed_file_forgets_the_history_of_the_list() {
    let files = [("Groceries.md", "- [ ] a\n")];
    let external = "- [ ] changed elsewhere\n";
    let expected_message = StatusMessage::info("nothing to undo");
    let mut test_app = TestApp::new(&files);
    test_app.press("tab x");
    test_app.write("Groceries.md", external);
    test_app.press("x");

    let system_under_test = test_app.press("u");

    assert_eq!(system_under_test.read("Groceries.md"), external);
    assert_eq!(
        system_under_test.app().status_message(),
        Some(&expected_message)
    );
}

#[test]
fn renamed_list_keeps_its_history() {
    let files = [("Groceries.md", "- [ ] a\n")];
    let mut test_app = TestApp::new(&files);
    test_app
        .press("tab x shift-tab r ctrl-u")
        .type_text("Food")
        .press("enter");

    let system_under_test = test_app.press("u u");

    assert_eq!(system_under_test.read("Groceries.md"), "- [ ] a\n");
}
