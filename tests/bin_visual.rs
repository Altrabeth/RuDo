//! Visual mode in the bin view: selecting several entries of the bin with `v` and restoring
//! them or deleting them permanently in one step.

#[expect(
    dead_code,
    reason = "each test crate uses only part of the shared harness"
)]
mod common;

use std::path::{Path, PathBuf};

use insta::assert_snapshot;
use ratatui::backend::TestBackend;
use ratatui::style::Modifier;
use rstest::rstest;
use rudo::app::{Mode, Severity, StatusMessage};
use rudo::config::keymap::action::Context;
use rudo::storage::bin::BinEntry;

use crate::common::TestApp;

/// What the index of the bin remembers: `Work` was deleted last, then `Home/Groceries.md`,
/// then `Old.md`.
const INDEX: &str = r#"[
  {"original": "Old.md", "location": "Old.md", "deleted_at": "2026-03-08T12:00:00Z"},
  {"original": "Home/Groceries.md", "location": "Home/Groceries.md", "deleted_at": "2026-03-10T12:00:00Z"},
  {"original": "Work", "location": "Work", "deleted_at": "2026-03-12T12:00:00Z"}
]"#;

const GROCERIES: &str = "# Groceries\n\n- [ ] Milk\n- [x] Eggs\n";

/// The tree shows `Archive/`, `Inbox` and the bin row. The bin view shows, in this order,
/// `Work` (one list, one task), `Home/Groceries` (two tasks), `Old` (one task) and `Note`, a
/// note that Obsidian put into the bin.
const WORKSPACE: &[(&str, &str)] = &[
    ("Archive/Keep.md", ""),
    ("Inbox.md", "- [ ] Call mum\n"),
    (".trash/Work/Plan.md", "- [ ] Write\n"),
    (".trash/Home/Groceries.md", GROCERIES),
    (".trash/Old.md", "- [ ] Paint\n"),
    (".trash/Note.md", "- [ ] Read\n"),
    (".trash/.rudo-bin.json", INDEX),
];

/// Opens the bin, the last row of the tree, at `Work`.
const OPEN_BIN: &str = "G enter";

/// Selects `Work`, `Home/Groceries` and `Old` in visual mode.
const SELECT_THREE: &str = "v j j";

fn exists(test_app: &TestApp, path: &str) -> bool {
    test_app.app().workspace().root().join(path).exists()
}

fn message_of(test_app: &TestApp) -> Option<&str> {
    test_app.app().status_message().map(StatusMessage::text)
}

fn confirm_question(test_app: &TestApp) -> Option<&str> {
    match test_app.app().mode() {
        Mode::Confirm(overlay) => Some(overlay.message()),
        _ => None,
    }
}

fn bin_locations(test_app: &TestApp) -> Vec<PathBuf> {
    test_app
        .app()
        .workspace()
        .bin_entries()
        .iter()
        .map(|entry| entry.location().to_path_buf())
        .collect()
}

/// Returns the locations the index of the bin remembers, in the order of the file.
fn indexed_locations(test_app: &TestApp) -> Vec<String> {
    let index: serde_json::Value =
        serde_json::from_str(&test_app.read(".trash/.rudo-bin.json")).unwrap();
    index
        .as_array()
        .unwrap()
        .iter()
        .map(|record| record["location"].as_str().unwrap().to_owned())
        .collect()
}

/// Returns the text of the bottom row.
fn bottom_row(backend: &TestBackend) -> String {
    let buffer = backend.buffer();
    let row = buffer.area.height - 1;
    (0..buffer.area.width)
        .map(|column| buffer[(column, row)].symbol())
        .collect()
}

fn bin_app() -> TestApp {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press(OPEN_BIN);
    test_app
}

#[rstest]
#[case::one_row("v", Some(1))]
#[case::three_rows(SELECT_THREE, Some(3))]
#[case::to_the_bottom("v G", Some(4))]
#[case::back_over_the_anchor("j v j k k", Some(2))]
#[case::left_with_v("v j v", None)]
#[case::left_with_esc("v j esc", None)]
fn navigation_extends_the_selection_over_the_bin(
    #[case] keys: &str,
    #[case] expected: Option<usize>,
) {
    let mut test_app = bin_app();

    let system_under_test = test_app.press(keys);

    assert_eq!(system_under_test.app().visual_selection_count(), expected);
}

#[test]
fn visual_mode_in_the_bin_has_its_own_context() {
    let mut test_app = bin_app();

    let system_under_test = test_app.press("v");

    assert_eq!(system_under_test.app().context(), Context::BinVisual);
    assert!(matches!(system_under_test.app().mode(), Mode::Visual));
}

#[test]
fn selection_is_drawn_and_the_bottom_bar_shows_the_keys_of_the_bin() {
    let mut test_app = bin_app();
    test_app.press(SELECT_THREE);

    let anchor_row = 1;
    let unselected_row = 4;
    let column = 5;

    let system_under_test = test_app.render(80, 10);

    assert_snapshot!(system_under_test);
    let buffer = system_under_test.buffer();
    assert!(
        buffer[(column, anchor_row)]
            .modifier
            .contains(Modifier::REVERSED)
    );
    assert!(
        !buffer[(column, unselected_row)]
            .modifier
            .contains(Modifier::REVERSED)
    );
    assert_eq!(
        bottom_row(&system_under_test).trim_end(),
        " -- VISUAL -- 3 selected  R restore  d delete  esc cancel"
    );
}

#[test]
fn restore_moves_every_selected_entry_back_in_one_step() {
    let mut test_app = bin_app();
    test_app.press(SELECT_THREE);

    let system_under_test = test_app.press("R");

    assert_eq!(system_under_test.read("Work/Plan.md"), "- [ ] Write\n");
    assert_eq!(system_under_test.read("Home/Groceries.md"), GROCERIES);
    assert_eq!(system_under_test.read("Old.md"), "- [ ] Paint\n");
    assert_eq!(bin_locations(system_under_test), [PathBuf::from("Note.md")]);
    assert_eq!(
        message_of(system_under_test),
        Some("restored 3 lists and folders")
    );
    assert!(matches!(system_under_test.app().mode(), Mode::Normal));
    assert_eq!(system_under_test.app().context(), Context::Bin);
}

#[test]
fn restore_of_a_selection_is_undone_and_redone_in_one_step() {
    let mut test_app = bin_app();
    test_app.press(SELECT_THREE).press("R u");
    let undone = [
        exists(&test_app, ".trash/Work/Plan.md"),
        exists(&test_app, ".trash/Home/Groceries.md"),
        exists(&test_app, ".trash/Old.md"),
        exists(&test_app, "Work"),
        exists(&test_app, "Home/Groceries.md"),
        exists(&test_app, "Old.md"),
    ];

    let system_under_test = test_app.press("ctrl-r");

    assert_eq!(undone, [true, true, true, false, false, false]);
    assert!(exists(system_under_test, "Work/Plan.md"));
    assert!(exists(system_under_test, "Home/Groceries.md"));
    assert!(exists(system_under_test, "Old.md"));
    assert_eq!(bin_locations(system_under_test), [PathBuf::from("Note.md")]);
}

#[test]
fn restore_skips_taken_paths_names_them_and_keeps_them_selected() {
    let mut files = WORKSPACE.to_vec();
    files.push(("Home/Groceries.md", "other\n"));
    let mut test_app = TestApp::new(&files);
    test_app.press(OPEN_BIN).press(SELECT_THREE);
    let expected_bin = [PathBuf::from("Home/Groceries.md"), PathBuf::from("Note.md")];

    let system_under_test = test_app.press("R");

    let message = system_under_test.app().status_message().unwrap();
    assert_eq!(message.severity(), Severity::Error);
    assert_eq!(
        message.text(),
        "restored 2 of 3; 1 path is taken: Home/Groceries.md"
    );
    assert_eq!(system_under_test.read("Home/Groceries.md"), "other\n");
    assert_eq!(
        system_under_test.read(".trash/Home/Groceries.md"),
        GROCERIES
    );
    assert_eq!(bin_locations(system_under_test), expected_bin);
    assert_eq!(indexed_locations(system_under_test), ["Home/Groceries.md"]);
    assert_eq!(system_under_test.app().visual_selection_count(), Some(1));
    assert_eq!(
        system_under_test
            .app()
            .selected_bin_entry()
            .map(BinEntry::location),
        Some(Path::new("Home/Groceries.md"))
    );
}

#[test]
fn restore_names_every_taken_path_and_keeps_the_skipped_entries_selected() {
    let mut files = WORKSPACE.to_vec();
    files.extend([("Work/Other.md", ""), ("Old.md", "other\n")]);
    let mut test_app = TestApp::new(&files);
    test_app.press(OPEN_BIN).press(SELECT_THREE);

    let system_under_test = test_app.press("R");

    assert_eq!(
        message_of(system_under_test),
        Some("restored 1 of 3; 2 paths are taken: Work, Old.md")
    );
    assert_eq!(system_under_test.app().visual_selection_count(), Some(2));
    assert_eq!(indexed_locations(system_under_test), ["Old.md", "Work"]);
}

#[test]
fn mixed_selection_asks_once_for_the_folder_of_the_foreign_entries() {
    let mut test_app = bin_app();
    test_app.press("G k v j R");
    let asked = matches!(test_app.app().mode(), Mode::Prompt(_));

    let system_under_test = test_app.type_text("Archive").press("enter");

    assert!(asked);
    assert_eq!(system_under_test.read("Old.md"), "- [ ] Paint\n");
    assert_eq!(system_under_test.read("Archive/Note.md"), "- [ ] Read\n");
    assert!(!exists(system_under_test, ".trash/Note.md"));
    assert!(matches!(system_under_test.app().mode(), Mode::Normal));
}

#[test]
fn restore_of_a_mixed_selection_is_one_undo_step() {
    let mut test_app = bin_app();
    test_app.press("G k v j R").press("enter");

    let system_under_test = test_app.press("u");

    assert!(exists(system_under_test, ".trash/Old.md"));
    assert!(exists(system_under_test, ".trash/Note.md"));
    assert!(!exists(system_under_test, "Old.md"));
    assert!(!exists(system_under_test, "Note.md"));
}

#[test]
fn delete_asks_with_the_counts_and_says_that_it_cannot_be_undone() {
    let mut test_app = bin_app();
    test_app.press(SELECT_THREE);

    let system_under_test = test_app.press("d");

    assert_eq!(
        confirm_question(system_under_test),
        Some("Delete 3 lists with 4 tasks and 1 folder permanently? This cannot be undone.")
    );
}

#[test]
fn delete_of_one_selected_entry_names_it() {
    let mut test_app = bin_app();
    test_app.press("j v");

    let system_under_test = test_app.press("d");

    assert_eq!(
        confirm_question(system_under_test),
        Some("Delete list \"Groceries\" with 2 tasks permanently? This cannot be undone.")
    );
}

#[test]
fn confirmed_delete_removes_the_selected_entries_for_good() {
    let mut test_app = bin_app();
    test_app.press(SELECT_THREE);

    let system_under_test = test_app.press("d y");

    assert!(!exists(system_under_test, ".trash/Work"));
    assert!(!exists(system_under_test, ".trash/Home"));
    assert!(!exists(system_under_test, ".trash/Old.md"));
    assert!(!exists(system_under_test, ".trash/.rudo-bin.json"));
    assert_eq!(bin_locations(system_under_test), [PathBuf::from("Note.md")]);
    assert_eq!(
        message_of(system_under_test),
        Some("deleted 3 entries permanently")
    );
    assert!(matches!(system_under_test.app().mode(), Mode::Normal));
}

#[test]
fn declined_delete_keeps_every_entry_and_ends_the_selection() {
    let mut test_app = bin_app();
    test_app.press(SELECT_THREE);

    let system_under_test = test_app.press("d n");

    assert!(exists(system_under_test, ".trash/Work/Plan.md"));
    assert!(exists(system_under_test, ".trash/Home/Groceries.md"));
    assert!(exists(system_under_test, ".trash/Old.md"));
    assert_eq!(bin_locations(system_under_test).len(), 4);
    assert!(matches!(system_under_test.app().mode(), Mode::Normal));
    assert_eq!(system_under_test.app().context(), Context::Bin);
}

#[test]
fn permanently_deleted_selection_leaves_the_undo_history() {
    let mut test_app = TestApp::new(&[
        ("Inbox.md", "- [ ] Call mum\n"),
        ("Later.md", ""),
        ("Zoo.md", ""),
    ]);
    test_app.press("d d y j d d y G enter v j d y");

    let system_under_test = test_app.press("u");

    assert_eq!(message_of(system_under_test), Some("nothing to undo"));
    assert!(!exists(system_under_test, "Inbox.md"));
    assert!(!exists(system_under_test, "Zoo.md"));
}

#[rstest]
#[case::yank("y")]
#[case::move_to_folder("m")]
fn selection_actions_of_the_panes_do_nothing_in_the_bin(#[case] key: &str) {
    let mut test_app = bin_app();
    test_app.press(SELECT_THREE);

    let system_under_test = test_app.press(key);

    assert!(matches!(system_under_test.app().mode(), Mode::Visual));
    assert_eq!(system_under_test.app().visual_selection_count(), Some(3));
    assert_eq!(bin_locations(system_under_test).len(), 4);
}

#[test]
fn restore_is_not_bound_in_visual_mode_of_the_panes() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press("v j");

    let system_under_test = test_app.press("R");

    assert_eq!(system_under_test.app().context(), Context::Visual);
    assert_eq!(bin_locations(system_under_test).len(), 4);
}

#[test]
fn leaving_the_bin_after_the_selection_ends_leaves_no_selection_behind() {
    let mut test_app = bin_app();

    let system_under_test = test_app.press("v j esc h");

    assert!(system_under_test.app().lists().bin_view().is_none());
    assert!(matches!(system_under_test.app().mode(), Mode::Normal));
    assert_eq!(system_under_test.app().context(), Context::Lists);
    assert_eq!(system_under_test.app().lists().outline().anchor(), None);
}

#[test]
fn restoring_everything_closes_the_bin_and_ends_visual_mode() {
    let mut test_app = bin_app();
    test_app.press("v G R");

    let system_under_test = test_app.press("enter");

    assert!(system_under_test.app().lists().bin_view().is_none());
    assert!(matches!(system_under_test.app().mode(), Mode::Normal));
    assert_eq!(system_under_test.app().context(), Context::Lists);
    assert!(exists(system_under_test, "Note.md"));
}

#[cfg(unix)]
#[test]
fn partly_failed_delete_reports_in_one_message_and_keeps_the_index_in_line() {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    /// Makes a folder read-only and writable again on drop, so that the temporary directory
    /// can be deleted.
    struct ReadOnly(PathBuf);

    impl Drop for ReadOnly {
        fn drop(&mut self) {
            fs::set_permissions(&self.0, fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    let mut test_app = bin_app();
    let locked = test_app.app().workspace().root().join(".trash/Work");
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o555)).unwrap();
    let _read_only = ReadOnly(locked);
    test_app.press("v j d");
    let expected_start = "deleted 1 of 2 entries permanently; 1 failed: cannot change ";

    let system_under_test = test_app.press("y");

    let message = system_under_test.app().status_message().unwrap();
    assert_eq!(message.severity(), Severity::Error);
    assert!(
        message.text().starts_with(expected_start),
        "{}",
        message.text()
    );
    assert!(!exists(system_under_test, ".trash/Home/Groceries.md"));
    assert!(exists(system_under_test, ".trash/Work/Plan.md"));
    assert_eq!(indexed_locations(system_under_test), ["Old.md", "Work"]);
    assert_eq!(
        bin_locations(system_under_test),
        [
            PathBuf::from("Work"),
            PathBuf::from("Old.md"),
            PathBuf::from("Note.md")
        ]
    );
}
