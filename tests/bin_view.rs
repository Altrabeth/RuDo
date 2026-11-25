//! The bin view of the lists pane: browsing the bin, restoring entries, deleting them
//! permanently and emptying the bin.

#[expect(
    dead_code,
    reason = "each test crate uses only part of the shared harness"
)]
mod common;

use std::path::{Path, PathBuf};

use insta::assert_snapshot;
use rstest::rstest;
use rudo::app::lists::EntryKey;
use rudo::app::{Focus, Mode, Severity, StatusMessage};
use rudo::config::keymap::action::Context;
use rudo::storage::bin::BinEntry;
use rudo::ui::help::{HelpHeading, HelpRowKey};

use crate::common::TestApp;

/// What the index of the bin remembers: `Work` was deleted last, `Home/Groceries.md` before.
const INDEX: &str = r#"[
  {"original": "Home/Groceries.md", "location": "Home/Groceries.md", "deleted_at": "2026-03-10T12:00:00Z"},
  {"original": "Work", "location": "Work", "deleted_at": "2026-03-12T12:00:00Z"}
]"#;

const GROCERIES: &str = "# Groceries\n\n- [ ] Milk\n- [x] Eggs\n";

/// The tree shows `Inbox` and the bin row. The bin view shows `Work` (one list, one task),
/// `Home/Groceries` (two tasks) and `Note`, a note that Obsidian put into the bin.
const WORKSPACE: &[(&str, &str)] = &[
    ("Inbox.md", "- [ ] Call mum\n"),
    (".trash/Home/Groceries.md", GROCERIES),
    (".trash/Work/Plan.md", "- [ ] Write\n"),
    (".trash/Note.md", "- [ ] Read\n"),
    (".trash/image.png", "png"),
    (".trash/.rudo-bin.json", INDEX),
];

/// Opens the bin from the start of [`WORKSPACE`].
const OPEN_BIN: &str = "j enter";

fn exists(test_app: &TestApp, path: &str) -> bool {
    test_app.app().workspace().root().join(path).exists()
}

fn message_of(test_app: &TestApp) -> Option<&str> {
    test_app.app().status_message().map(StatusMessage::text)
}

fn selected_location(test_app: &TestApp) -> Option<&Path> {
    test_app.app().selected_bin_entry().map(BinEntry::location)
}

fn confirm_question(test_app: &TestApp) -> Option<&str> {
    match test_app.app().mode() {
        Mode::Confirm(overlay) => Some(overlay.message()),
        _ => None,
    }
}

/// Returns the text of row `y` of a rendering of `test_app` at 80 by 16 cells.
fn rendered_row(test_app: &mut TestApp, y: u16) -> String {
    let backend = test_app.render(80, 16);
    (0..80).map(|x| backend.buffer()[(x, y)].symbol()).collect()
}

#[test]
fn tree_ends_with_the_bin_and_the_number_of_its_entries() {
    let mut test_app = TestApp::new(WORKSPACE);

    let system_under_test = test_app.render(80, 8);

    assert_snapshot!(system_under_test);
}

#[test]
fn empty_bin_has_no_row() {
    let test_app = TestApp::new(&[("Inbox.md", ""), (".trash/.rudo-bin.json", "[]")]);
    let expected = [EntryKey::List(test_app.list_path("Inbox.md"))];

    let system_under_test: Vec<EntryKey> = test_app
        .app()
        .lists()
        .items(test_app.app().workspace())
        .iter()
        .map(|item| match item.entry() {
            Some(entry) => EntryKey::from(entry),
            None => EntryKey::Bin(PathBuf::from(".trash")),
        })
        .collect();

    assert_eq!(system_under_test, expected);
}

#[test]
fn open_shows_the_entries_with_their_original_path_and_deletion_date() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press(OPEN_BIN);

    let system_under_test = test_app.render(80, 10);

    assert_snapshot!(system_under_test);
    assert_eq!(test_app.app().context(), Context::Bin);
}

#[rstest]
#[case::expand("j l")]
#[case::open("j enter")]
fn expand_and_open_open_the_bin_at_the_newest_entry(#[case] keys: &str) {
    let mut test_app = TestApp::new(WORKSPACE);
    let expected = Path::new("Work");

    let system_under_test = test_app.press(keys);

    assert_eq!(selected_location(system_under_test), Some(expected));
}

#[rstest]
#[case::h("h")]
#[case::left("left")]
#[case::esc("esc")]
fn close_bin_returns_to_the_tree_on_the_bin_row(#[case] key: &str) {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press(OPEN_BIN);
    let expected = EntryKey::Bin(PathBuf::from(".trash"));

    let system_under_test = test_app.press(key);

    assert!(system_under_test.app().lists().bin_view().is_none());
    assert_eq!(
        system_under_test.app().lists().outline().selected(),
        Some(&expected)
    );
    assert_eq!(system_under_test.app().context(), Context::Lists);
}

#[test]
fn selected_list_shows_its_tasks_read_only_with_a_hint() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press(OPEN_BIN).press("j");

    let system_under_test = test_app.render(80, 10);

    assert_snapshot!(system_under_test);
}

#[test]
fn selected_folder_says_what_it_holds() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press(OPEN_BIN);

    let system_under_test = rendered_row(&mut test_app, 1);

    assert!(
        system_under_test.contains("In the bin: folder \"Work\" with 1 list and 1 task."),
        "{system_under_test}"
    );
}

#[test]
fn editing_keys_never_reach_a_list_in_the_bin() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press(OPEN_BIN).press("j");

    let system_under_test = test_app.press("x o a i 2 tab enter space y y p > J D r");

    assert_eq!(
        system_under_test.read(".trash/Home/Groceries.md"),
        GROCERIES
    );
    assert_eq!(system_under_test.read("Inbox.md"), "- [ ] Call mum\n");
    assert_eq!(system_under_test.app().focus(), Focus::Lists);
    assert!(matches!(system_under_test.app().mode(), Mode::Normal));
    assert!(system_under_test.app().lists().bin_view().is_some());
}

#[test]
fn restore_moves_the_entry_back_to_its_original_path() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press(OPEN_BIN).press("j");

    let system_under_test = test_app.press("R");

    assert_eq!(system_under_test.read("Home/Groceries.md"), GROCERIES);
    assert!(!exists(system_under_test, ".trash/Home"));
    assert_eq!(message_of(system_under_test), Some("restored Groceries"));
    assert_eq!(
        selected_location(system_under_test),
        Some(Path::new("Note.md"))
    );
    assert!(
        system_under_test
            .app()
            .workspace()
            .entry(Path::new("Home/Groceries.md"))
            .is_some()
    );
}

#[test]
fn restore_is_undone_and_redone_like_any_change() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press(OPEN_BIN).press("j R u");
    let undone = [
        exists(&test_app, "Home/Groceries.md"),
        exists(&test_app, ".trash/Home/Groceries.md"),
    ];

    let system_under_test = test_app.press("ctrl-r");

    assert_eq!(undone, [false, true]);
    assert_eq!(system_under_test.read("Home/Groceries.md"), GROCERIES);
    assert!(!exists(system_under_test, ".trash/Home/Groceries.md"));
}

#[test]
fn restore_onto_a_taken_path_is_refused_and_keeps_the_entry_in_the_bin() {
    let mut test_app = TestApp::new(&[
        ("Home/Groceries.md", "other\n"),
        (".trash/Home/Groceries.md", GROCERIES),
        (".trash/.rudo-bin.json", INDEX),
    ]);
    test_app.press("j j enter");

    let system_under_test = test_app.press("R");

    let message = system_under_test.app().status_message().unwrap();
    assert_eq!(message.severity(), Severity::Error);
    assert!(
        message.text().starts_with("cannot restore: ")
            && message
                .text()
                .ends_with("already exists; rename or move it first"),
        "{}",
        message.text()
    );
    assert_eq!(system_under_test.read("Home/Groceries.md"), "other\n");
    assert_eq!(
        system_under_test.read(".trash/Home/Groceries.md"),
        GROCERIES
    );
    assert_eq!(
        selected_location(system_under_test),
        Some(Path::new("Home/Groceries.md"))
    );
}

#[test]
fn restore_of_a_foreign_entry_asks_for_the_folder() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press(OPEN_BIN).press("G R");

    let system_under_test = test_app.type_text("Archive").press("enter");

    let Mode::Prompt(prompt) = system_under_test.app().mode() else {
        panic!("expected the prompt to stay open");
    };
    assert_eq!(prompt.prompt().error(), Some("there is no folder Archive"));
}

#[test]
fn restore_of_a_foreign_entry_into_the_top_level() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press(OPEN_BIN).press("G R");

    let system_under_test = test_app.press("enter");

    assert_eq!(system_under_test.read("Note.md"), "- [ ] Read\n");
    assert!(!exists(system_under_test, ".trash/Note.md"));
    assert!(matches!(system_under_test.app().mode(), Mode::Normal));
}

#[test]
fn delete_asks_and_says_that_it_cannot_be_undone() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press(OPEN_BIN);

    let system_under_test = test_app.press("d d");

    assert_eq!(
        confirm_question(system_under_test),
        Some("Delete folder \"Work\" with 1 list and 1 task permanently? This cannot be undone.")
    );
}

#[test]
fn confirmed_delete_removes_the_entry_for_good() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press(OPEN_BIN);

    let system_under_test = test_app.press("d d y");

    assert!(!exists(system_under_test, ".trash/Work"));
    assert_eq!(
        message_of(system_under_test),
        Some("deleted Work permanently")
    );
    assert_eq!(
        selected_location(system_under_test),
        Some(Path::new("Home/Groceries.md"))
    );
}

#[test]
fn permanent_deletion_is_not_undone() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press(OPEN_BIN).press("d d y");

    let system_under_test = test_app.press("u");

    assert_eq!(message_of(system_under_test), Some("nothing to undo"));
    assert!(!exists(system_under_test, ".trash/Work"));
    assert!(!exists(system_under_test, "Work"));
}

#[test]
fn permanently_deleted_entry_leaves_the_undo_history() {
    let mut test_app = TestApp::new(&[("Inbox.md", "- [ ] Call mum\n"), ("Later.md", "")]);
    test_app.press("d d y j enter d d y");

    let system_under_test = test_app.press("u");

    assert_eq!(message_of(system_under_test), Some("nothing to undo"));
    assert!(!exists(system_under_test, "Inbox.md"));
}

#[test]
fn deleted_list_shows_up_in_the_bin_at_once() {
    let mut test_app = TestApp::new(&[("Inbox.md", "- [ ] Call mum\n"), ("Later.md", "")]);
    test_app.press("d d y j enter");

    let system_under_test = selected_location(&test_app);

    assert_eq!(system_under_test, Some(Path::new("Inbox.md")));
}

#[test]
fn empty_bin_asks_with_the_number_of_lists_and_tasks() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press(OPEN_BIN);

    let system_under_test = test_app.press("E");

    assert_eq!(
        confirm_question(system_under_test),
        Some("Delete 3 lists with 4 tasks permanently? This cannot be undone.")
    );
}

#[test]
fn confirmed_empty_bin_deletes_everything_and_returns_to_the_tree() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press(OPEN_BIN);

    let system_under_test = test_app.press("E y");

    let bin = system_under_test.app().workspace().root().join(".trash");
    assert_eq!(std::fs::read_dir(bin).unwrap().count(), 0);
    assert_eq!(message_of(system_under_test), Some("emptied the bin"));
    assert!(system_under_test.app().lists().bin_view().is_none());
    assert!(system_under_test.app().workspace().bin_entries().is_empty());
}

#[test]
fn declined_empty_bin_keeps_everything() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press(OPEN_BIN);

    let system_under_test = test_app.press("E n");

    assert!(exists(system_under_test, ".trash/Note.md"));
    assert_eq!(system_under_test.app().workspace().bin_entries().len(), 3);
}

#[test]
fn bin_with_foreign_files_and_a_corrupt_index_still_shows_them() {
    let test_app = TestApp::new(&[
        ("Inbox.md", ""),
        (".trash/Note.md", "- [ ] Read\n"),
        (".trash/.rudo-bin.json", "{ corrupt"),
    ]);
    let expected = [(PathBuf::from("Note.md"), None)];

    let system_under_test: Vec<(PathBuf, Option<PathBuf>)> = test_app
        .app()
        .workspace()
        .bin_entries()
        .iter()
        .map(|entry| {
            (
                entry.location().to_path_buf(),
                entry.original().map(Path::to_path_buf),
            )
        })
        .collect();

    assert_eq!(system_under_test, expected);
}

#[test]
fn help_opens_at_the_keys_of_the_bin() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press(OPEN_BIN);
    let expected = HelpRowKey::Heading(HelpHeading::Bin);

    let system_under_test = test_app.press("?");

    let Mode::Help(help) = system_under_test.app().mode() else {
        panic!("expected the help screen");
    };
    assert_eq!(help.selected(), Some(expected));
}

#[test]
fn visual_selection_never_moves_the_bin() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press("G O").type_text("Archive").press("enter");
    let selected_with_the_bin = test_app.press("g g j v G").app().visual_selection_count();

    let system_under_test = test_app.press("m").type_text("Archive").press("enter");

    assert_eq!(selected_with_the_bin, Some(1));
    assert!(exists(system_under_test, "Archive/Inbox.md"));
    assert!(exists(system_under_test, ".trash/Note.md"));
    assert!(!exists(system_under_test, "Archive/.trash"));
}

#[test]
fn deleting_the_last_entry_selects_the_entry_before_the_bin() {
    let mut test_app = TestApp::new(&[("Home/Chores.md", ""), ("Inbox.md", "")]);
    let expected = EntryKey::List(test_app.list_path("Home/Chores.md"));

    let system_under_test = test_app.press("G d d y");

    assert_eq!(
        system_under_test.app().lists().outline().selected(),
        Some(&expected)
    );
}

#[test]
fn bottom_bar_shows_the_keys_of_the_bin() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press(OPEN_BIN);

    let system_under_test = rendered_row(&mut test_app, 15);

    assert!(
        system_under_test.contains("R restore")
            && system_under_test.contains("dd delete")
            && system_under_test.contains("E empty bin"),
        "{system_under_test}"
    );
}
