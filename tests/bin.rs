//! Deleting lists and folders into the bin, and undoing and redoing every change of lists and
//! folders with `u` and `ctrl-r`.

#[expect(
    dead_code,
    reason = "each test crate uses only part of the shared harness"
)]
mod common;

use std::path::{Path, PathBuf};

use rudo::app::lists::EntryKey;
use rudo::app::{Severity, StatusMessage};
use rudo::config::Config;
use rudo::storage::watch::{FsChange, PathState};
use rudo::storage::workspace::Entry;

use crate::common::TestApp;

/// Front matter, prose and odd spacing that a restore must keep byte for byte.
const CHORES: &str = "---\ntags: [home]\n---\n# Chores\n\n- [ ] Vacuum  twice\n* [ ] Dust\n";

/// Rows in display order: `Home/` (`Chores`, `Groceries`), `Inbox`. `Home/Chores` is selected
/// and active at the start.
const WORKSPACE: &[(&str, &str)] = &[
    ("Inbox.md", "- [ ] Call mum\n"),
    ("Home/Chores.md", CHORES),
    ("Home/Groceries.md", "- [ ] Milk\n"),
];

fn exists(test_app: &TestApp, path: &str) -> bool {
    test_app.app().workspace().root().join(path).exists()
}

fn selected(test_app: &TestApp) -> Option<&EntryKey> {
    test_app.app().lists().outline().selected()
}

fn message_of(test_app: &TestApp) -> Option<&str> {
    test_app.app().status_message().map(StatusMessage::text)
}

#[test]
fn delete_moves_the_list_into_the_bin_with_its_path() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press("d d y");

    let system_under_test = test_app.read(".trash/Home/Chores.md");

    assert_eq!(system_under_test, CHORES);
    assert!(!exists(&test_app, "Home/Chores.md"));
    assert_eq!(test_app.app().lists().active(), None);
}

#[test]
fn undo_of_a_delete_restores_the_bytes_the_selection_and_the_active_list() {
    let mut test_app = TestApp::new(WORKSPACE);
    let chores = test_app.list_path("Home/Chores.md");
    let expected_key = EntryKey::List(chores.clone());
    test_app.press("d d y");

    let system_under_test = test_app.press("u");

    assert_eq!(system_under_test.read("Home/Chores.md"), CHORES);
    assert!(!exists(system_under_test, ".trash/Home"));
    assert_eq!(selected(system_under_test), Some(&expected_key));
    assert_eq!(system_under_test.app().lists().active(), Some(&chores));
    assert_eq!(message_of(system_under_test), Some("undo: Chores"));
}

#[test]
fn undo_of_a_deleted_non_empty_folder_restores_everything_inside() {
    let mut test_app = TestApp::new(WORKSPACE);
    let expected_key = EntryKey::Folder(PathBuf::from("Home"));
    test_app.press("g g d d y");

    let system_under_test = test_app.press("u");

    assert_eq!(system_under_test.read("Home/Chores.md"), CHORES);
    assert_eq!(system_under_test.read("Home/Groceries.md"), "- [ ] Milk\n");
    assert_eq!(selected(system_under_test), Some(&expected_key));
    assert!(
        system_under_test
            .app()
            .workspace()
            .entry(Path::new("Home/Groceries.md"))
            .is_some()
    );
}

#[test]
fn redo_moves_the_restored_entry_into_the_bin_again() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press("d d y u");

    let system_under_test = test_app.press("ctrl-r");

    assert!(!exists(system_under_test, "Home/Chores.md"));
    assert_eq!(system_under_test.read(".trash/Home/Chores.md"), CHORES);
    assert_eq!(message_of(system_under_test), Some("redo: Chores"));
}

#[test]
fn undo_of_a_bulk_delete_is_one_step() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press("j v j d y");

    let system_under_test = test_app.press("u");

    assert!(exists(system_under_test, "Home/Groceries.md"));
    assert!(exists(system_under_test, "Inbox.md"));
    assert_eq!(
        message_of(system_under_test),
        Some("undo: 2 lists and folders")
    );
}

#[test]
fn deleting_the_same_name_twice_keeps_both_in_the_bin() {
    let mut test_app = TestApp::new(&[("Inbox.md", "first\n")]);
    test_app.press("d d y o").type_text("Inbox").press("enter");
    test_app.write("Inbox.md", "second\n");
    test_app.press("1 d d y");

    let system_under_test = [
        test_app.read(".trash/Inbox.md"),
        test_app.read(".trash/Inbox 2.md"),
    ];

    assert_eq!(system_under_test, ["first\n", "second\n"]);
}

#[test]
fn restore_onto_a_taken_path_fails_and_keeps_the_entry_in_the_bin() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press("d d y");
    test_app.write("Home/Chores.md", "other\n");
    test_app.press("u");

    let system_under_test = test_app.app().status_message().unwrap();

    assert_eq!(system_under_test.severity(), Severity::Error);
    assert!(
        system_under_test.text().starts_with("cannot undo: ")
            && system_under_test.text().ends_with("already exists"),
        "{}",
        system_under_test.text()
    );
    assert_eq!(test_app.read("Home/Chores.md"), "other\n");
    assert_eq!(test_app.read(".trash/Home/Chores.md"), CHORES);
}

#[test]
fn failed_restore_stays_undoable_once_the_path_is_free() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press("d d y");
    test_app.write("Home/Chores.md", "other\n");
    test_app.press("u");
    test_app.remove("Home/Chores.md");

    let system_under_test = test_app.press("u");

    assert_eq!(system_under_test.read("Home/Chores.md"), CHORES);
}

#[test]
fn task_history_survives_a_delete_and_restore() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press("tab x shift-tab d d y");

    let system_under_test = test_app.press("u u");

    assert_eq!(system_under_test.read("Home/Chores.md"), CHORES);
}

#[test]
fn undo_of_a_new_list_moves_it_into_the_bin_and_redo_brings_it_back() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press("G o").type_text("Garden").press("enter u");
    let undone = exists(&test_app, "Garden.md");

    let system_under_test = test_app.press("ctrl-r");

    assert!(!undone);
    assert!(exists(system_under_test, "Garden.md"));
    assert!(!exists(system_under_test, ".trash/Garden.md"));
}

#[test]
fn undo_of_a_rename_restores_the_name_and_redo_renames_again() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app
        .press("G r ctrl-u")
        .type_text("Calls")
        .press("enter u");
    let undone = exists(&test_app, "Inbox.md");

    let system_under_test = test_app.press("ctrl-r");

    assert!(undone);
    assert!(exists(system_under_test, "Calls.md"));
    assert!(!exists(system_under_test, "Inbox.md"));
}

#[test]
fn undo_of_a_move_puts_the_lists_back() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press("G O").type_text("Archive").press("enter");
    test_app
        .press("j j j v j m")
        .type_text("Archive")
        .press("enter u");

    let system_under_test = [
        exists(&test_app, "Home/Groceries.md"),
        exists(&test_app, "Inbox.md"),
        exists(&test_app, "Archive/Inbox.md"),
    ];

    assert_eq!(system_under_test, [true, true, false]);
}

#[test]
fn configured_bin_receives_deletions_and_stays_out_of_the_tree() {
    let config = Config::from_yaml("workspace:\n  bin: Bin\n", Path::new("config.yaml")).unwrap();
    let mut test_app = TestApp::with_config(&[("Bin/Old.md", ""), ("Inbox.md", "")], config);
    test_app.press("d d y");

    let system_under_test = test_app.app().workspace().entries();

    assert!(system_under_test.is_empty());
    assert!(exists(&test_app, "Bin/Inbox.md"));
}

#[test]
fn changes_inside_the_bin_are_ignored_by_the_live_reload() {
    let config = Config::from_yaml("workspace:\n  bin: Bin\n", Path::new("config.yaml")).unwrap();
    let test_app = TestApp::with_config(&[("Inbox.md", "")], config);
    let changes = [
        FsChange::new("Bin", PathState::Directory),
        FsChange::new("Bin/Inbox.md", PathState::File),
    ];

    let system_under_test = test_app.app().on_fs_changed(&changes);

    assert_eq!(system_under_test, []);
}

#[test]
fn configured_hidden_bin_stays_out_of_the_tree_when_hidden_entries_are_shown() {
    let yaml = "workspace:\n  bin: .deleted\n";
    let config = Config::from_yaml(yaml, Path::new("config.yaml")).unwrap();
    let files = [
        (".deleted/Old.md", ""),
        (".notes/Ideas.md", ""),
        ("Inbox.md", ""),
    ];
    let mut test_app = TestApp::with_config(&files, config);
    test_app.press("H");

    let system_under_test: Vec<&Path> = test_app
        .app()
        .workspace()
        .entries()
        .iter()
        .map(Entry::path)
        .collect();

    assert_eq!(
        system_under_test,
        [Path::new(".notes"), Path::new("Inbox.md")]
    );
}
