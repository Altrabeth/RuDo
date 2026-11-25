//! Following changes that other programs make to the workspace: reloading changed lists,
//! scanning the tree again and resolving save conflicts in favour of the file on disk.

#[expect(
    dead_code,
    reason = "each test crate uses only part of the shared harness"
)]
mod common;

use rudo::app::{Mode, StatusMessage};
use rudo::storage::watch::{FsChange, PathState};

use crate::common::TestApp;

/// Two tasks; the first one has a subtask.
const GROCERIES: &str = "- [ ] Buy milk\n\t- [ ] Oat milk\n- [ ] Bake bread\n";

/// The same number of tasks as [`GROCERIES`] at the same lines, all of them different.
const CHANGED_GROCERIES: &str = "- [ ] Buy candles\n\t- [ ] Tea lights\n- [ ] Buy matches\n";

const TASK_GONE: &str = "the edited task no longer exists";

/// Opens `Groceries.md` with [`GROCERIES`] and focuses the tasks pane.
fn groceries() -> TestApp {
    let mut test_app = TestApp::new(&[("Groceries.md", GROCERIES)]);
    test_app.press("2");
    test_app
}

/// Delivers the change of `path` to the app, as the file watcher does.
fn notify(test_app: &mut TestApp, path: &str, state: PathState) {
    let changes = [FsChange::new(path, state)];
    test_app.run(|app| app.on_fs_changed(&changes));
}

/// Writes `content` to the file at `path` as another program would and reports it.
fn write_externally(test_app: &mut TestApp, path: &str, content: &str) {
    test_app.write(path, content);
    notify(test_app, path, PathState::File);
}

/// Returns the description of the selected task of the active list.
fn selected_task(test_app: &TestApp) -> String {
    let app = test_app.app();
    let task = *app.task_outline().unwrap().selected().unwrap();
    let loaded = app.active_list().unwrap().content().unwrap();
    let node = loaded
        .document()
        .tasks()
        .find(|node| node.id() == task)
        .unwrap();
    node.task().description().to_owned()
}

/// Returns the text of the active list as RuDo holds it.
fn active_document(test_app: &TestApp) -> String {
    let app = test_app.app();
    let loaded = app.active_list().unwrap().content().unwrap();
    loaded.document().to_string()
}

#[test]
fn list_written_by_another_program_is_reloaded_with_a_message() {
    let mut test_app = groceries();
    let expected_message = StatusMessage::info("Groceries reloaded");
    write_externally(&mut test_app, "Groceries.md", CHANGED_GROCERIES);

    let system_under_test = active_document(&test_app);

    assert_eq!(system_under_test, CHANGED_GROCERIES);
    assert_eq!(test_app.app().status_message(), Some(&expected_message));
}

#[test]
fn reload_keeps_the_selection_on_the_line_of_the_selected_task() {
    let mut test_app = groceries();
    test_app.press("j j");
    write_externally(&mut test_app, "Groceries.md", CHANGED_GROCERIES);

    let system_under_test = selected_task(&test_app);

    assert_eq!(system_under_test, "Buy matches");
}

#[test]
fn own_save_reported_by_the_watcher_changes_nothing() {
    let mut test_app = groceries();
    test_app.press("j x");
    let expected_document = active_document(&test_app);
    notify(&mut test_app, "Groceries.md", PathState::File);

    let system_under_test = test_app.app().status_message();

    assert_eq!(system_under_test, None);
    assert_eq!(active_document(&test_app), expected_document);
    assert_eq!(selected_task(&test_app), "Oat milk");
}

#[test]
fn inline_edit_stays_open_across_a_reload() {
    let mut test_app = groceries();
    test_app.press("i").type_text(" now");
    write_externally(&mut test_app, "Groceries.md", CHANGED_GROCERIES);

    let system_under_test = test_app.app().mode();

    match system_under_test {
        Mode::InlineEdit(edit) => assert_eq!(edit.input().value(), "Buy milk now"),
        mode => panic!("the inline editor was closed, the mode is {mode:?}"),
    }
}

#[test]
fn edit_of_a_task_changed_on_disk_shows_an_error_instead_of_editing_another_task() {
    let mut test_app = groceries();
    let expected_message = StatusMessage::error(TASK_GONE);
    test_app.press("j j i").type_text(" now");
    write_externally(&mut test_app, "Groceries.md", CHANGED_GROCERIES);

    let system_under_test = test_app.press("enter");

    assert_eq!(system_under_test.read("Groceries.md"), CHANGED_GROCERIES);
    assert_eq!(active_document(system_under_test), CHANGED_GROCERIES);
    assert_eq!(
        system_under_test.app().status_message(),
        Some(&expected_message)
    );
    assert!(matches!(system_under_test.app().mode(), Mode::Normal));
}

#[test]
fn due_date_of_a_task_changed_on_disk_shows_an_error_instead_of_changing_another_task() {
    let mut test_app = groceries();
    let expected_message = StatusMessage::error(TASK_GONE);
    test_app.press("D").type_text("tomorrow");
    write_externally(&mut test_app, "Groceries.md", CHANGED_GROCERIES);

    let system_under_test = test_app.press("enter");

    assert_eq!(system_under_test.read("Groceries.md"), CHANGED_GROCERIES);
    assert_eq!(
        system_under_test.app().status_message(),
        Some(&expected_message)
    );
    assert!(matches!(system_under_test.app().mode(), Mode::Normal));
}

#[test]
fn new_task_in_a_list_removed_on_disk_shows_an_error() {
    let mut test_app = TestApp::new(&[("Groceries.md", GROCERIES), ("Work.md", "")]);
    test_app.press("2 o").type_text("Buy eggs");
    test_app.remove("Groceries.md");
    notify(&mut test_app, "Groceries.md", PathState::Missing);
    let expected_message = StatusMessage::error("the edited list no longer exists");

    let system_under_test = test_app.press("enter");

    assert_eq!(
        system_under_test.app().status_message(),
        Some(&expected_message)
    );
    assert!(matches!(system_under_test.app().mode(), Mode::Normal));
}

#[test]
fn save_of_a_list_changed_on_disk_keeps_the_disk_version_and_reloads_it() {
    let mut test_app = groceries();
    test_app.write("Groceries.md", CHANGED_GROCERIES);
    let expected_message =
        StatusMessage::error("Groceries changed on disk; your last change was not applied");

    let system_under_test = test_app.press("x");

    assert_eq!(system_under_test.read("Groceries.md"), CHANGED_GROCERIES);
    assert_eq!(active_document(system_under_test), CHANGED_GROCERIES);
    assert_eq!(
        system_under_test.app().status_message(),
        Some(&expected_message)
    );
    assert_eq!(selected_task(system_under_test), "Buy candles");
}

#[test]
fn undo_after_a_save_conflict_has_nothing_to_undo() {
    let mut test_app = groceries();
    test_app.press("j x");
    test_app.write("Groceries.md", CHANGED_GROCERIES);
    test_app.press("x");
    let expected_message = StatusMessage::info("nothing to undo");

    let system_under_test = test_app.press("u");

    assert_eq!(system_under_test.read("Groceries.md"), CHANGED_GROCERIES);
    assert_eq!(
        system_under_test.app().status_message(),
        Some(&expected_message)
    );
}

#[test]
fn rescan_keeps_the_selection_and_folds_of_an_unchanged_list() {
    let mut test_app = groceries();
    test_app.press("z a j");
    let expected_screen = test_app.render(80, 24).to_string();
    notify(&mut test_app, "", PathState::Directory);

    let system_under_test = test_app.render(80, 24).to_string();

    assert_eq!(system_under_test, expected_screen);
    assert_eq!(selected_task(&test_app), "Bake bread");
}

#[test]
fn rescan_adds_a_new_list_and_keeps_the_open_one() {
    let mut test_app = groceries();
    test_app.press("j");
    write_externally(&mut test_app, "Work.md", "- [ ] Write report\n");
    let expected_active = test_app.list_path("Groceries.md");

    let system_under_test = test_app.app();

    assert_eq!(system_under_test.lists().active(), Some(&expected_active));
    assert!(
        system_under_test
            .workspace()
            .list(&test_app.list_path("Work.md"))
            .is_some()
    );
    assert_eq!(selected_task(&test_app), "Oat milk");
}

#[test]
fn rescan_keeps_the_undo_history_of_an_unchanged_list() {
    let mut test_app = groceries();
    test_app.press("x");
    write_externally(&mut test_app, "Work.md", "");

    let system_under_test = test_app.press("u");

    assert_eq!(system_under_test.read("Groceries.md"), GROCERIES);
}

#[test]
fn rescan_reloads_a_list_whose_file_changed_and_forgets_its_history() {
    let mut test_app = groceries();
    test_app.press("x");
    test_app.write("Groceries.md", CHANGED_GROCERIES);
    notify(&mut test_app, "", PathState::Directory);

    let system_under_test = test_app.press("u");

    assert_eq!(active_document(system_under_test), CHANGED_GROCERIES);
    assert_eq!(system_under_test.read("Groceries.md"), CHANGED_GROCERIES);
}

#[test]
fn removed_active_list_empties_the_tasks_pane_with_a_message() {
    let mut test_app = TestApp::new(&[("Groceries.md", GROCERIES), ("Work.md", "")]);
    test_app.press("2");
    test_app.remove("Groceries.md");
    let expected_message = StatusMessage::info("Groceries was removed on disk");
    notify(&mut test_app, "Groceries.md", PathState::Missing);

    let system_under_test = test_app.app();

    assert_eq!(system_under_test.lists().active(), None);
    assert!(system_under_test.task_outline().is_none());
    assert_eq!(system_under_test.status_message(), Some(&expected_message));
}
