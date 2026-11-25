//! End-to-end user journeys from the documentation, compared byte for byte against the golden
//! Markdown files in `tests/fixtures/expected/`.
//!
//! Every journey starts from the fixtures in `tests/fixtures/`, copied into a fresh `TempDir`
//! workspace by [`TestApp`], and runs on the fixed date [`common::TODAY`] (Saturday,
//! 2026-03-14) with a configuration that never comes from the user's environment.

#[expect(
    dead_code,
    reason = "each test crate uses only part of the shared harness"
)]
mod common;

use std::path::Path;

use insta::assert_snapshot;
use rstest::rstest;
use rudo::app::{Mode, StatusMessage};
use rudo::config::Config;

use crate::common::TestApp;

/// A list with a tag, a priority, subtasks, a recurring task and every status. Its rows in
/// display order: `Buy milk` (`Check lactose-free` done, `Oat milk`), `Bake bread` done,
/// `Water plants` due today every week, `Buy candles` cancelled.
const GROCERIES: &str = include_str!("fixtures/groceries.md");

/// [`GROCERIES`] after another program ticked `Oat milk` and added `Buy eggs`.
const GROCERIES_CHANGED_ON_DISK: &str = include_str!("fixtures/groceries_changed_on_disk.md");

/// An Obsidian note with front matter, headings, prose and a code block holding a line that
/// looks like a task. Its rows: `Buy milk` (`Check lactose-free`), `Bake bread`, `Buy screws`.
const OBSIDIAN_NOTE: &str = include_str!("fixtures/obsidian_note.md");

/// A list indented with tabs: `Plan summer trip` (`Book flights`), `Call mum`.
const INBOX: &str = include_str!("fixtures/inbox.md");

/// A list indented with two spaces: `Renovate kitchen` (`Pick tiles`).
const PROJECTS: &str = include_str!("fixtures/projects.md");

/// Rebinds `toggle-done` to `ctrl-x`, `new-task-below` to `n` and `help` to `f1`.
const CUSTOM_CONFIG: &str = include_str!("fixtures/custom_config.yaml");

/// Opens a workspace holding only `Groceries.md` with `content` and focuses its tasks.
fn groceries_with(content: &str) -> TestApp {
    let mut test_app = TestApp::new(&[("Groceries.md", content)]);
    test_app.press("2");
    test_app
}

#[test]
fn first_run_creates_a_list_with_tasks_subtasks_and_their_fields() {
    let expected = include_str!("fixtures/expected/first_run.md");
    let mut test_app = TestApp::new(&[]);

    let system_under_test = test_app
        .press("o")
        .type_text("Groceries")
        .press("enter enter o")
        .type_text("Buy milk")
        .press("enter a")
        .type_text("Check lactose-free")
        .press("enter x k D")
        .type_text("tomorrow")
        .press("enter + + o")
        .type_text("Water plants")
        .press("enter r")
        .type_text("every week")
        .press("enter");

    assert_eq!(system_under_test.read("Groceries.md"), expected);
}

/// Returns `text` as an editor on Windows saves it: with a byte order mark and CRLF.
fn with_bom_and_crlf(text: &str) -> String {
    format!("\u{feff}{}", text.replace('\n', "\r\n"))
}

#[rstest]
#[case::as_written(str::to_owned)]
#[case::with_a_byte_order_mark_and_crlf(with_bom_and_crlf)]
fn obsidian_note_changes_only_its_task_lines(#[case] encode: fn(&str) -> String) {
    let expected = encode(include_str!("fixtures/expected/obsidian_note.md"));
    let mut test_app = groceries_with(&encode(OBSIDIAN_NOTE));

    let system_under_test = test_app
        .press("j j x G i")
        .type_text(" for shelves")
        .press("enter o")
        .type_text("Buy nails")
        .press("enter g g D ctrl-u")
        .type_text("+3d")
        .press("enter");

    assert_eq!(system_under_test.read("Groceries.md"), expected);
}

/// Moves `Plan summer trip` with its subtask from `Inbox` below `Renovate kitchen` in
/// `Projects`, with `dd` and `p`.
fn move_between_lists() -> TestApp {
    let mut test_app = TestApp::new(&[("Inbox.md", INBOX), ("Projects.md", PROJECTS)]);
    test_app.press("2 d d 1 j enter g g p");
    test_app
}

#[test]
fn delete_and_paste_moves_a_task_with_its_subtasks_to_another_list() {
    let expected_inbox = include_str!("fixtures/expected/inbox_after_move.md");
    let expected_projects = include_str!("fixtures/expected/projects_after_move.md");

    let system_under_test = move_between_lists();

    assert_eq!(system_under_test.read("Inbox.md"), expected_inbox);
    assert_eq!(system_under_test.read("Projects.md"), expected_projects);
}

#[test]
fn undoing_a_move_between_lists_restores_both_lists() {
    let mut test_app = move_between_lists();

    let system_under_test = test_app.press("u u");

    assert_eq!(system_under_test.read("Inbox.md"), INBOX);
    assert_eq!(system_under_test.read("Projects.md"), PROJECTS);
}

/// Toggles, indents, adds, raises the priority, sets a due date and deletes: six changes.
fn undo_redo_chain() -> TestApp {
    let mut test_app = groceries_with(OBSIDIAN_NOTE);
    test_app
        .press("j j x > o")
        .type_text("Buy eggs")
        .press("enter + D")
        .type_text("fri")
        .press("enter G d d");
    test_app
}

#[test]
fn undo_chain_restores_the_original_bytes() {
    let mut test_app = undo_redo_chain();

    let system_under_test = test_app.press("u u u u u u");

    assert_eq!(system_under_test.read("Groceries.md"), OBSIDIAN_NOTE);
}

#[test]
fn redo_chain_restores_the_changed_bytes() {
    let expected = include_str!("fixtures/expected/undo_redo.md");
    let mut test_app = undo_redo_chain();
    test_app.press("u u u u u u");

    let system_under_test = test_app.press("ctrl-r ctrl-r ctrl-r ctrl-r ctrl-r ctrl-r");

    assert_eq!(system_under_test.read("Groceries.md"), expected);
}

/// Opens [`GROCERIES`] with [`CUSTOM_CONFIG`] and focuses its tasks.
fn custom_config() -> TestApp {
    let config = Config::from_yaml(CUSTOM_CONFIG, Path::new("config.yaml")).unwrap();
    let mut test_app = TestApp::with_config(&[("Groceries.md", GROCERIES)], config);
    test_app.press("2");
    test_app
}

#[test]
fn custom_configuration_changes_the_keys() {
    let expected = include_str!("fixtures/expected/custom_config.md");
    let mut test_app = custom_config();

    let system_under_test = test_app
        .press("x ctrl-x n")
        .type_text("Buy bread")
        .press("enter");

    assert_eq!(system_under_test.read("Groceries.md"), expected);
}

#[rstest]
#[case::help_key("help screen")]
#[case::new_task_key("add a task")]
#[case::done_key("mark as done")]
fn custom_configuration_changes_the_help_screen(#[case] query: &str) {
    let snapshot_name = format!("help_filtered_by_{}", query.replace(' ', "_"));
    let mut test_app = custom_config();
    test_app.press("f1 /").type_text(query);

    let system_under_test = test_app.render(80, 24);

    assert_snapshot!(snapshot_name, system_under_test);
}

#[test]
fn search_finds_the_task_to_complete() {
    let expected = include_str!("fixtures/expected/search.md");
    let mut test_app = groceries_with(GROCERIES);

    let system_under_test = test_app.press("/").type_text("oat").press("enter G x");

    assert_eq!(system_under_test.read("Groceries.md"), expected);
}

#[test]
fn hidden_completed_tasks_stay_in_the_file() {
    let expected = include_str!("fixtures/expected/hide_completed.md");
    let mut test_app = groceries_with(GROCERIES);

    let system_under_test = test_app
        .press("H j x o")
        .type_text("Buy flour")
        .press("enter");

    assert_eq!(system_under_test.read("Groceries.md"), expected);
}

#[test]
fn completing_recurring_tasks_inserts_their_next_occurrences() {
    let expected = include_str!("fixtures/expected/recurrence.md");
    let mut test_app = groceries_with(GROCERIES);

    let system_under_test = test_app
        .press("G k x g g j j r")
        .type_text("every 2 days")
        .press("enter x");

    assert_eq!(system_under_test.read("Groceries.md"), expected);
}

/// Opens [`GROCERIES`], lets another program write [`GROCERIES_CHANGED_ON_DISK`] before the
/// watcher reports it, and completes `Buy milk`.
fn save_conflict() -> TestApp {
    let mut test_app = groceries_with(GROCERIES);
    test_app.write("Groceries.md", GROCERIES_CHANGED_ON_DISK);
    test_app.press("x");
    test_app
}

#[test]
fn save_conflict_keeps_the_file_on_disk_and_says_so() {
    let expected_message =
        StatusMessage::error("Groceries changed on disk; your last change was not applied");

    let system_under_test = save_conflict();

    assert_eq!(
        system_under_test.read("Groceries.md"),
        GROCERIES_CHANGED_ON_DISK
    );
    assert_eq!(
        system_under_test.app().status_message(),
        Some(&expected_message)
    );
    assert!(matches!(system_under_test.app().mode(), Mode::Normal));
}

#[test]
fn change_after_a_save_conflict_applies_to_the_file_on_disk() {
    let expected = include_str!("fixtures/expected/conflict.md");
    let mut test_app = save_conflict();

    let system_under_test = test_app.press("x");

    assert_eq!(system_under_test.read("Groceries.md"), expected);
}
