//! Showing and hiding hidden folders and lists with `H` in the lists pane.

#[expect(
    dead_code,
    reason = "each test crate uses only part of the shared harness"
)]
mod common;

use std::path::Path;

use insta::assert_snapshot;
use ratatui::backend::TestBackend;
use ratatui::style::{Color, Modifier, Style};
use rstest::rstest;
use rudo::app::entry_visibility::HiddenEntries;
use rudo::app::task_visibility::CompletedTasks;
use rudo::config::Config;
use rudo::storage::watch::{FsChange, PathState};

use crate::common::TestApp;

/// Rows in display order while hidden entries are shown: `.obsidian/` (`templates/` with
/// `Daily`), `Home/` (`Chores`), `.drafts`, `Inbox`, then the row of the bin. Only `Home/`,
/// `Chores`, `Inbox` and the bin are visible; `.git/` has no list and `.trash/` is the bin, so
/// neither is ever part of the tree.
const WORKSPACE: &[(&str, &str)] = &[
    (".obsidian/workspace.json", "{}"),
    (".obsidian/templates/Daily.md", "- [ ] Plan the day\n"),
    (".drafts.md", "- [ ] Draft a letter\n"),
    ("Home/Chores.md", "- [ ] Vacuum\n- [x] Dust\n"),
    ("Inbox.md", "- [ ] Call mum\n"),
    (".git/objects/ab/cdef", "blob"),
    (".trash/Old.md", "- [ ] Forgotten\n"),
];

/// The names of the rows of the lists pane in display order.
const VISIBLE_ROWS: &[&str] = &["Home", "Chores", "Inbox"];

/// The names of the rows of the lists pane in display order while hidden entries are shown.
const ALL_ROWS: &[&str] = &[
    ".obsidian",
    "templates",
    "Daily",
    "Home",
    "Chores",
    ".drafts",
    "Inbox",
];

/// Styles of the default theme.
const FOLDER: Style = Style::new().fg(Color::Blue);
const HIDDEN_FOLDER: Style = FOLDER.add_modifier(Modifier::DIM);
const LIST: Style = Style::new().fg(Color::Reset);
const HIDDEN_LIST: Style = LIST.add_modifier(Modifier::DIM);

/// Returns the colour and modifiers of the cell at `x`, `y`; the background is never set.
fn style_at(backend: &TestBackend, x: u16, y: u16) -> Style {
    let cell = &backend.buffer()[(x, y)];
    Style::new().fg(cell.fg).add_modifier(cell.modifier)
}

/// Returns the names of the entries the lists pane shows, in display order, without the row
/// of the bin.
fn row_names(test_app: &TestApp) -> Vec<String> {
    let app = test_app.app();
    app.lists()
        .items(app.workspace())
        .iter()
        .filter_map(|item| item.entry().map(|entry| entry.name().to_owned()))
        .collect()
}

/// Returns the name of the selected entry of the lists pane.
fn selected_name(test_app: &TestApp) -> Option<String> {
    let selected = test_app.app().lists().outline().selected()?;
    let name = selected.path().file_stem()?;
    Some(name.to_string_lossy().into_owned())
}

/// Returns the top row of a rendering 80 columns wide, which holds the pane titles.
fn top_row(test_app: &mut TestApp) -> String {
    let backend = test_app.render(80, 12);
    (0..80).map(|x| backend.buffer()[(x, 0)].symbol()).collect()
}

/// Delivers the change of `path` to the app, as the file watcher does.
fn notify<'app>(test_app: &'app mut TestApp, path: &str, state: PathState) -> &'app TestApp {
    let changes = [FsChange::new(path, state)];
    test_app.run(|app| app.on_fs_changed(&changes))
}

#[test]
fn hidden_entries_are_hidden_on_start_up() {
    let test_app = TestApp::new(WORKSPACE);

    let system_under_test = row_names(&test_app);

    assert_eq!(system_under_test, VISIBLE_ROWS);
}

#[test]
fn h_shows_hidden_folders_and_lists_in_display_order() {
    let mut test_app = TestApp::new(WORKSPACE);

    let system_under_test = test_app.press("H");

    assert_eq!(row_names(system_under_test), ALL_ROWS);
    assert_eq!(
        system_under_test.app().hidden_entries(),
        HiddenEntries::Shown
    );
}

#[test]
fn shown_hidden_entries_are_listed_and_named_in_the_title() {
    let mut test_app = TestApp::new(WORKSPACE);

    let system_under_test = test_app.press("H").render(80, 12);

    assert_snapshot!(system_under_test);
}

#[rstest]
#[case::hidden_folder(4, 1, HIDDEN_FOLDER)]
#[case::folder_in_a_hidden_folder(6, 2, HIDDEN_FOLDER)]
#[case::list_in_a_hidden_folder(7, 3, HIDDEN_LIST)]
#[case::visible_folder(4, 4, FOLDER)]
#[case::hidden_list(4, 6, HIDDEN_LIST)]
#[case::visible_list(4, 7, LIST)]
fn hidden_entries_are_dimmed(#[case] x: u16, #[case] y: u16, #[case] expected: Style) {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press("H");

    let system_under_test = test_app.render(80, 12);

    assert_eq!(style_at(&system_under_test, x, y), expected);
}

#[test]
fn title_says_hidden_shown_while_they_are_shown() {
    let mut test_app = TestApp::new(WORKSPACE);
    let expected_title = "Lists  hidden shown";

    let system_under_test = top_row(test_app.press("H"));

    assert!(system_under_test.contains(expected_title));
}

#[test]
fn h_twice_hides_them_again() {
    let mut test_app = TestApp::new(WORKSPACE);

    let system_under_test = test_app.press("H H");

    assert_eq!(row_names(system_under_test), VISIBLE_ROWS);
    assert!(!top_row(system_under_test).contains("hidden shown"));
}

#[rstest]
#[case::visible_entry_stays_selected("G k H", "Inbox")]
#[case::hidden_list_hands_over_to_the_nearest_entry("G k k H", "Inbox")]
#[case::entry_in_a_hidden_folder_hands_over_to_the_next("g g j j H", "Home")]
#[case::hidden_folder_hands_over_to_the_next("g g H", "Home")]
fn hiding_again_keeps_the_selection_sensible(#[case] keys: &str, #[case] expected: &str) {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press("H");

    let system_under_test = test_app.press(keys);

    assert_eq!(selected_name(system_under_test).as_deref(), Some(expected));
}

#[test]
fn showing_hidden_entries_keeps_the_selection() {
    let mut test_app = TestApp::new(WORKSPACE);
    let expected = Some("Inbox".to_owned());

    let system_under_test = test_app.press("G k H");

    assert_eq!(selected_name(system_under_test), expected);
}

#[test]
fn h_in_the_tasks_pane_still_toggles_completed_tasks() {
    let mut test_app = TestApp::new(WORKSPACE);

    let system_under_test = test_app.press("2 H");

    assert_eq!(
        system_under_test.app().completed_tasks(),
        CompletedTasks::Hidden
    );
    assert_eq!(
        system_under_test.app().hidden_entries(),
        HiddenEntries::Hidden
    );
    assert_eq!(row_names(system_under_test), VISIBLE_ROWS);
}

#[test]
fn h_in_the_lists_pane_leaves_completed_tasks_alone() {
    let mut test_app = TestApp::new(WORKSPACE);

    let system_under_test = test_app.press("H");

    assert_eq!(
        system_under_test.app().completed_tasks(),
        CompletedTasks::Shown
    );
}

#[test]
fn show_hidden_setting_starts_with_them_shown() {
    let config =
        Config::from_yaml("workspace:\n  show_hidden: true\n", Path::new("c.yaml")).unwrap();

    let mut system_under_test = TestApp::with_config(WORKSPACE, config);

    assert_eq!(row_names(&system_under_test), ALL_ROWS);
    assert!(top_row(&mut system_under_test).contains("hidden shown"));
}

#[test]
fn the_bin_never_appears() {
    let mut test_app = TestApp::new(WORKSPACE);
    let bin = Path::new(".trash");

    let system_under_test = test_app.press("H");

    assert!(system_under_test.app().workspace().entry(bin).is_none());
    assert!(!row_names(system_under_test).contains(&".trash".to_owned()));
}

#[test]
fn a_list_in_a_shown_hidden_folder_opens() {
    let mut test_app = TestApp::new(WORKSPACE);
    let expected = test_app.list_path(".obsidian/templates/Daily.md");

    let system_under_test = test_app.press("H g g j j enter");

    assert_eq!(system_under_test.app().lists().active(), Some(&expected));
}

#[rstest]
#[case::hidden("", &[])]
#[case::shown("H", &[".obsidian", "templates", "Daily"])]
fn search_finds_hidden_lists_only_while_they_are_shown(
    #[case] keys: &str,
    #[case] expected: &[&str],
) {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press("/").type_text("dai").press("enter");

    let system_under_test = test_app.press(keys);

    assert_eq!(row_names(system_under_test), expected);
}

#[test]
fn a_new_list_in_a_hidden_folder_appears_after_the_watcher_reports_it() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press("H");
    test_app.write(".obsidian/templates/Weekly.md", "- [ ] Review\n");
    let expected = [
        ".obsidian",
        "templates",
        "Daily",
        "Weekly",
        "Home",
        "Chores",
        ".drafts",
        "Inbox",
    ];

    let system_under_test = notify(
        &mut test_app,
        ".obsidian/templates/Weekly.md",
        PathState::File,
    );

    assert_eq!(row_names(system_under_test), expected);
}

#[test]
fn a_new_list_in_the_bin_never_appears() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press("H");
    test_app.write(".trash/Deleted.md", "- [ ] Gone\n");

    let system_under_test = notify(&mut test_app, ".trash/Deleted.md", PathState::File);

    assert_eq!(row_names(system_under_test), ALL_ROWS);
}

#[test]
fn visual_mode_deletes_a_shown_hidden_list_with_a_visible_one() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press("H G k k");

    let system_under_test = test_app.press("v j d y");

    assert_eq!(row_names(system_under_test), &ALL_ROWS[..5]);
}
