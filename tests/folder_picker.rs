//! The folder picker: choosing the target folder of `m` and of restoring a bin entry of
//! unknown origin by typing a few letters, with `tab` completion.

#[expect(
    dead_code,
    reason = "each test crate uses only part of the shared harness"
)]
mod common;

use std::fs;
use std::path::Path;

use insta::assert_snapshot;
use ratatui::backend::TestBackend;
use rstest::rstest;
use rudo::app::folder_picker::FolderPicker;
use rudo::app::picker::PickerCandidate;
use rudo::app::{Mode, StatusMessage};
use rudo::config::Config;
use rudo::config::keymap::action::Context;
use rudo::storage::watch::{FsChange, PathState};

use crate::common::TestApp;

/// Folders in display order: `Home`, `Home/Garden`, `Release Notes`, `Release Notes/Old`,
/// `Work`, `Work/Projects`, `Work/Projects/Release` and the hidden `.archive`; the list
/// `Inbox` is the last row of the tree, selected with `G`.
const WORKSPACE: &[(&str, &str)] = &[
    ("Home/Garden/Seeds.md", "- [ ] Sow\n"),
    ("Release Notes/Old/Readme.md", ""),
    ("Work/Projects/Release/Notes.md", ""),
    (".archive/Old.md", ""),
    ("Inbox.md", "- [ ] Call mum\n"),
];

/// Selects `Inbox` and opens the folder picker to move it.
const MOVE_INBOX: &str = "G m";

/// A bin with `Note`, a note that another program put there, so its origin is unknown.
const BIN_WORKSPACE: &[(&str, &str)] = &[
    ("Archive/Keep.md", ""),
    ("Inbox.md", ""),
    (".trash/Note.md", "- [ ] Read\n"),
];

/// Opens the bin, the last row of the tree, and restores `Note`.
const RESTORE_NOTE: &str = "G enter R";

fn folder_picker_of(test_app: &TestApp) -> &FolderPicker {
    match test_app.app().mode() {
        Mode::FolderPicker(folder_picker) => folder_picker,
        mode => panic!("no folder picker is open, the mode is {mode:?}"),
    }
}

fn results_of(test_app: &TestApp) -> Vec<&str> {
    folder_picker_of(test_app)
        .picker()
        .results(0)
        .map(|result| result.candidate().text())
        .collect()
}

fn input_of(test_app: &TestApp) -> &str {
    folder_picker_of(test_app).picker().input().value()
}

fn exists(test_app: &TestApp, path: &str) -> bool {
    test_app.app().workspace().root().join(path).exists()
}

fn message_of(test_app: &TestApp) -> Option<&str> {
    test_app.app().status_message().map(StatusMessage::text)
}

/// Returns the text of the bottom row.
fn bottom_row(backend: &TestBackend) -> String {
    let buffer = backend.buffer();
    let row = buffer.area.height - 1;
    (0..buffer.area.width)
        .map(|column| buffer[(column, row)].symbol())
        .collect()
}

#[test]
fn empty_input_lists_the_root_and_every_shown_folder() {
    let mut test_app = TestApp::new(WORKSPACE);
    let expected = [
        "/",
        "Home",
        "Home/Garden",
        "Release Notes",
        "Release Notes/Old",
        "Work",
        "Work/Projects",
        "Work/Projects/Release",
    ];

    let system_under_test = test_app.press(MOVE_INBOX);

    assert_eq!(results_of(system_under_test), expected);
    assert_eq!(system_under_test.app().context(), Context::Picker);
}

#[test]
fn hidden_folders_are_candidates_while_they_are_shown() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press("H");

    let system_under_test = test_app.press(MOVE_INBOX).type_text("arch");

    assert_eq!(results_of(system_under_test), [".archive"]);
}

#[test]
fn typing_ranks_names_before_paths() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press(MOVE_INBOX);

    let system_under_test = test_app.type_text("rel");

    assert_eq!(
        results_of(system_under_test),
        [
            "Release Notes",
            "Work/Projects/Release",
            "Release Notes/Old"
        ]
    );
}

#[test]
fn picker_popup_shows_the_ranked_matches_and_its_keys() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press(MOVE_INBOX).type_text("rel");

    let system_under_test = test_app.render(80, 24);

    assert_snapshot!(system_under_test);
}

#[test]
fn enter_moves_into_the_best_match() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press(MOVE_INBOX).type_text("gar");

    let system_under_test = test_app.press("enter");

    assert_eq!(
        system_under_test.read("Home/Garden/Inbox.md"),
        "- [ ] Call mum\n"
    );
    assert!(!exists(system_under_test, "Inbox.md"));
    assert!(matches!(system_under_test.app().mode(), Mode::Normal));
}

#[rstest]
#[case::down("down")]
#[case::ctrl_n("ctrl-n")]
fn next_selects_the_following_match(#[case] key: &str) {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press(MOVE_INBOX).type_text("rel").press(key);

    let system_under_test = test_app.press("enter");

    assert!(exists(system_under_test, "Work/Projects/Release/Inbox.md"));
}

#[rstest]
#[case::up("up")]
#[case::ctrl_p("ctrl-p")]
fn previous_selects_the_match_before(#[case] key: &str) {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press(MOVE_INBOX).press("down down").press(key);

    let system_under_test = test_app.press("enter");

    assert!(exists(system_under_test, "Home/Inbox.md"));
}

#[test]
fn tab_completes_the_selected_folder_and_cycles() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press(MOVE_INBOX).type_text("rel");
    let completions: Vec<String> = (0..4)
        .map(|_| input_of(test_app.press("tab")).to_owned())
        .collect();

    let system_under_test = completions;

    assert_eq!(
        system_under_test,
        [
            "Release Notes",
            "Work/Projects/Release",
            "Release Notes/Old",
            "Release Notes"
        ]
    );
}

#[test]
fn typing_after_tab_ranks_again() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press(MOVE_INBOX).type_text("gar").press("tab");

    let system_under_test = test_app.press("backspace backspace backspace backspace backspace");

    assert_eq!(input_of(system_under_test), "Home/G");
    assert_eq!(results_of(system_under_test), ["Home/Garden"]);
}

#[test]
fn pasted_text_filters_the_folders() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press(MOVE_INBOX);

    let system_under_test = test_app.run(|app| app.handle_paste("projects"));

    assert_eq!(
        results_of(system_under_test),
        ["Work/Projects", "Work/Projects/Release"]
    );
}

#[test]
fn smart_case_respects_capitals() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press(MOVE_INBOX);

    let system_under_test = test_app.type_text("RN");

    assert_eq!(
        results_of(system_under_test),
        ["Release Notes", "Release Notes/Old"]
    );
}

#[test]
fn without_a_match_enter_reports_the_missing_folder() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press(MOVE_INBOX).type_text("Garage");

    let system_under_test = test_app.press("enter");

    assert_eq!(results_of(system_under_test), Vec::<&str>::new());
    assert_eq!(
        folder_picker_of(system_under_test).error(),
        Some("there is no folder Garage")
    );
    assert!(exists(system_under_test, "Inbox.md"));
}

#[test]
fn picker_without_a_match_says_so() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app
        .press(MOVE_INBOX)
        .type_text("Garage")
        .press("enter");

    let system_under_test = test_app.render(80, 24);

    assert_snapshot!(system_under_test);
}

#[test]
fn esc_cancels_without_moving() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press(MOVE_INBOX).type_text("gar");

    let system_under_test = test_app.press("esc");

    assert!(matches!(system_under_test.app().mode(), Mode::Normal));
    assert!(exists(system_under_test, "Inbox.md"));
}

#[test]
fn printable_keys_go_to_the_input() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press(MOVE_INBOX);

    let system_under_test = test_app.press("j k q");

    assert_eq!(input_of(system_under_test), "jkq");
    assert!(!system_under_test.app().should_quit());
}

#[test]
fn rebound_complete_key_completes() {
    let config = Config::from_yaml(
        "keybindings:\n  complete: ctrl-l\n",
        Path::new("config.yaml"),
    )
    .unwrap();
    let mut test_app = TestApp::with_config(WORKSPACE, config);
    test_app.press(MOVE_INBOX).type_text("gar");

    let system_under_test = test_app.press("ctrl-l");

    assert_eq!(input_of(system_under_test), "Home/Garden");
}

#[test]
fn bottom_bar_shows_the_keys_of_the_picker() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press(MOVE_INBOX);

    let system_under_test = bottom_row(&test_app.render(80, 24));

    assert!(
        system_under_test.starts_with(" tab complete  enter accept  esc cancel"),
        "{system_under_test}"
    );
}

#[test]
fn moving_a_folder_into_itself_is_refused() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press("g g m").type_text("garden");

    let system_under_test = test_app.press("enter");

    assert_eq!(
        folder_picker_of(system_under_test).error(),
        Some("cannot move folder Home into itself")
    );
}

#[test]
fn restore_of_a_foreign_entry_offers_the_folders_without_the_bin() {
    let mut test_app = TestApp::new(BIN_WORKSPACE);

    let system_under_test = test_app.press(RESTORE_NOTE);

    assert_eq!(results_of(system_under_test), ["/", "Archive"]);
    assert_eq!(
        folder_picker_of(system_under_test).title(),
        "Restore into folder"
    );
}

#[test]
fn restore_of_a_foreign_entry_into_the_chosen_folder() {
    let mut test_app = TestApp::new(BIN_WORKSPACE);
    test_app.press(RESTORE_NOTE).type_text("arc");

    let system_under_test = test_app.press("enter");

    assert_eq!(system_under_test.read("Archive/Note.md"), "- [ ] Read\n");
    assert!(!exists(system_under_test, ".trash/Note.md"));
    assert!(matches!(system_under_test.app().mode(), Mode::Normal));
    assert_eq!(message_of(system_under_test), Some("restored Note"));
}

#[test]
fn restore_picker_explains_why_it_asks() {
    let mut test_app = TestApp::new(BIN_WORKSPACE);
    test_app.press(RESTORE_NOTE);

    let system_under_test = test_app.render(80, 24);

    assert_snapshot!(system_under_test);
}

#[test]
fn folder_removed_while_the_picker_is_open_is_reported() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press(MOVE_INBOX).type_text("gar");
    fs::remove_dir_all(test_app.app().workspace().root().join("Home/Garden")).unwrap();
    let changes = [FsChange::new("Home/Garden", PathState::Missing)];
    test_app.run(|app| app.on_fs_changed(&changes));

    let system_under_test = test_app.press("enter");

    assert_eq!(
        folder_picker_of(system_under_test).error(),
        Some("there is no folder Home/Garden")
    );
}
