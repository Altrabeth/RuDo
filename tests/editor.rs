//! Opening a list in an external editor with `e` and reloading it when the editor exits.
//!
//! The editor itself needs a terminal, so these tests check the effect that asks for it and
//! simulate the editor by changing the file as another program would.

#[expect(
    dead_code,
    reason = "each test crate uses only part of the shared harness"
)]
mod common;

use std::num::NonZeroUsize;

use rudo::app::{Effect, StatusMessage};
use rudo::config::keymap::key::KeyChord;

use crate::common::TestApp;

/// `Groceries` with the tasks `Buy milk` on line 3 and `Bake bread` on line 4.
const GROCERIES: &str = "# Groceries\n\n- [ ] Buy milk\n- [ ] Bake bread\n";

/// Presses `e` and returns the effects it asks for, without performing them.
fn press_e(test_app: &mut TestApp) -> Vec<Effect> {
    let chord: KeyChord = "e".parse().unwrap();
    let mut effects = Vec::new();
    test_app.run(|app| {
        effects = app.handle_key(chord);
        Vec::new()
    });
    effects
}

/// Returns the description of the task selected in the tasks pane.
fn selected_task(test_app: &TestApp) -> String {
    let app = test_app.app();
    let task = *app.task_outline().unwrap().selected().unwrap();
    let document = app.active_list().unwrap().content().unwrap().document();
    let node = document.tasks().find(|node| node.id() == task).unwrap();
    node.task().description().to_owned()
}

#[test]
fn e_in_the_tasks_pane_opens_the_list_at_the_selected_task() {
    let mut test_app = TestApp::new(&[("Groceries.md", GROCERIES)]);
    test_app.press("tab j");
    let expected = Effect::OpenInEditor {
        list: test_app.list_path("Groceries.md"),
        line: NonZeroUsize::new(4),
    };

    let system_under_test = press_e(&mut test_app);

    assert_eq!(system_under_test, [expected]);
}

#[test]
fn e_in_the_lists_pane_opens_the_selected_list_at_its_start() {
    let mut test_app = TestApp::new(&[("Groceries.md", GROCERIES), ("Work.md", "")]);
    test_app.press("j");
    let expected = Effect::OpenInEditor {
        list: test_app.list_path("Work.md"),
        line: None,
    };

    let system_under_test = press_e(&mut test_app);

    assert_eq!(system_under_test, [expected]);
}

#[test]
fn list_changed_in_the_editor_is_reloaded_with_the_selection_on_its_line() {
    let mut test_app = TestApp::new(&[("Groceries.md", GROCERIES)]);
    test_app.press("tab j");
    let list = test_app.list_path("Groceries.md");
    test_app.write(
        "Groceries.md",
        "# Groceries\n\n- [ ] Buy oat milk\n- [x] Bake rye bread\n- [ ] Buy candles\n",
    );
    let expected_document =
        "# Groceries\n\n- [ ] Buy oat milk\n- [x] Bake rye bread\n- [ ] Buy candles\n";
    test_app.run(|app| {
        app.reload_list(&list);
        Vec::new()
    });

    let system_under_test = test_app.app().active_list().unwrap().content().unwrap();

    assert_eq!(system_under_test.document().to_string(), expected_document);
    assert_eq!(selected_task(&test_app), "Bake rye bread");
    assert_eq!(
        test_app.app().status_message(),
        Some(&StatusMessage::info("Groceries reloaded"))
    );
}

#[test]
fn list_left_unchanged_in_the_editor_keeps_its_selection() {
    let mut test_app = TestApp::new(&[("Groceries.md", GROCERIES)]);
    test_app.press("tab j");
    let list = test_app.list_path("Groceries.md");
    let selected_before = *test_app.app().task_outline().unwrap().selected().unwrap();
    test_app.run(|app| {
        app.reload_list(&list);
        Vec::new()
    });

    let system_under_test = test_app.app().task_outline().unwrap().selected();

    assert_eq!(system_under_test, Some(&selected_before));
    assert_eq!(test_app.read("Groceries.md"), GROCERIES);
}

#[test]
fn edits_after_a_reload_are_saved_on_top_of_the_editor_changes() {
    let mut test_app = TestApp::new(&[("Groceries.md", GROCERIES)]);
    let list = test_app.list_path("Groceries.md");
    test_app.write("Groceries.md", "- [ ] Buy milk\n- [ ] Buy candles\n");
    let expected = "- [ ] Buy milk\n- [x] Buy candles\n";
    test_app.run(|app| {
        app.reload_list(&list);
        Vec::new()
    });
    test_app.run(|app| {
        app.edit_list(&list, |document| {
            let task = document.tasks().nth(1).unwrap().id();
            document.toggle_done(task, None)
        })
    });

    let system_under_test = test_app.read("Groceries.md");

    assert_eq!(system_under_test, expected);
}
