//! The lists pane: navigating the workspace tree, opening lists and what the pane shows.

#[expect(
    dead_code,
    reason = "each test crate uses only part of the shared harness"
)]
mod common;

use std::path::PathBuf;

use insta::assert_snapshot;
use ratatui::backend::TestBackend;
use ratatui::style::{Color, Modifier, Style};
use rstest::rstest;
use rudo::app::Focus;
use rudo::app::lists::EntryKey;

use crate::common::TestApp;

/// Rows in display order: `Home/` (`Chores`, `Groceries`), `Work/` (`Plan`), `Inbox`.
const WORKSPACE: &[(&str, &str)] = &[
    ("Inbox.md", "- [ ] Call mum\n"),
    (
        "Home/Groceries.md",
        "- [ ] Buy milk\n\t- [x] Check lactose-free\n- [ ] Bake bread\n- [ ] Water plants\n",
    ),
    ("Home/Chores.md", "# Chores\n\n- [ ] Vacuum\n- [ ] Dust\n"),
    ("Work/Plan.md", "- [x] Write report\n"),
];

/// Styles of the default theme.
const FOLDER: Style = Style::new().fg(Color::Blue);
const LIST_COUNT: Style = Style::new().fg(Color::DarkGray);
const UNREADABLE: Style = Style::new()
    .fg(Color::LightRed)
    .add_modifier(Modifier::BOLD);
const SELECTION: Style = Style::new()
    .fg(Color::Reset)
    .add_modifier(Modifier::REVERSED);
const SELECTION_UNFOCUSED: Style = Style::new()
    .fg(Color::Reset)
    .add_modifier(Modifier::REVERSED.union(Modifier::DIM));

/// The key of the entry at `path`; folders are written with a trailing `/`.
fn key(test_app: &TestApp, path: &str) -> EntryKey {
    match path.strip_suffix('/') {
        Some(folder) => EntryKey::Folder(PathBuf::from(folder)),
        None => EntryKey::List(test_app.list_path(path)),
    }
}

/// A workspace of `count` lists named `list-00` and up, without folders.
fn many_lists(count: usize) -> TestApp {
    let paths: Vec<String> = (0..count)
        .map(|index| format!("list-{index:02}.md"))
        .collect();
    let files: Vec<(&str, &str)> = paths.iter().map(|path| (path.as_str(), "")).collect();
    TestApp::new(&files)
}

/// Returns the text of row `y` of the lists pane of an 80 columns wide terminal.
fn lists_row(backend: &TestBackend, y: u16) -> String {
    (1..23).map(|x| backend.buffer()[(x, y)].symbol()).collect()
}

/// Returns the colour and modifiers of the cell at `x`, `y`; the background is never set.
fn style_at(backend: &TestBackend, x: u16, y: u16) -> Style {
    let cell = &backend.buffer()[(x, y)];
    Style::new().fg(cell.fg).add_modifier(cell.modifier)
}

#[test]
fn start_selects_and_opens_the_first_list() {
    let test_app = TestApp::new(WORKSPACE);
    let expected_key = key(&test_app, "Home/Chores.md");
    let expected_active = test_app.list_path("Home/Chores.md");

    let system_under_test = test_app.app();

    assert_eq!(
        system_under_test.lists().outline().selected(),
        Some(&expected_key)
    );
    assert_eq!(system_under_test.lists().active(), Some(&expected_active));
    assert_eq!(system_under_test.focus(), Focus::Lists);
}

#[test]
fn start_without_lists_opens_nothing() {
    let test_app = TestApp::new(&[]);

    let system_under_test = test_app.app().lists();

    assert_eq!(system_under_test.outline().selected(), None);
    assert_eq!(system_under_test.active(), None);
}

#[rstest]
#[case::down("j", "Home/Groceries.md")]
#[case::up("j k", "Home/Chores.md")]
#[case::bottom("G", "Inbox.md")]
#[case::top("G g g", "Home/")]
#[case::collapse_a_list_selects_its_folder("h", "Home/")]
#[case::collapsed_folder_hides_its_lists("h h j", "Work/")]
#[case::toggle_fold("g g z a j", "Work/")]
#[case::toggle_fold_twice("g g z a z a j", "Home/Chores.md")]
#[case::expand_a_collapsed_folder("g g z a l j", "Home/Chores.md")]
#[case::fold_all_selects_the_folder("z M", "Home/")]
#[case::fold_all("z M j j", "Inbox.md")]
#[case::unfold_all("z M z R j", "Home/Chores.md")]
#[case::open_a_folder_folds_it("g g enter j", "Work/")]
#[case::open_a_folder_twice("g g enter enter j", "Home/Chores.md")]
#[case::keys_in_the_tasks_pane_do_not_move("2 j G", "Home/Chores.md")]
fn keys_move_the_selection_and_fold(#[case] keys: &str, #[case] expected: &str) {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press(keys);
    let expected_key = key(&test_app, expected);

    let system_under_test = test_app.app().lists().outline().selected();

    assert_eq!(system_under_test, Some(&expected_key));
}

#[rstest]
#[case::open("j enter")]
#[case::expand("j l")]
#[case::right_arrow("j right")]
fn opening_a_list_activates_it_and_focuses_the_tasks_pane(#[case] keys: &str) {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press(keys);
    let expected_active = test_app.list_path("Home/Groceries.md");

    let system_under_test = test_app.app();

    assert_eq!(system_under_test.lists().active(), Some(&expected_active));
    assert_eq!(system_under_test.focus(), Focus::Tasks);
}

#[rstest]
#[case::move_without_opening("j")]
#[case::open_a_folder("g g enter")]
#[case::expand_a_folder("g g z a l")]
#[case::expand_an_expanded_folder("g g l")]
fn other_keys_keep_the_active_list_and_the_focus(#[case] keys: &str) {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press(keys);
    let expected_active = test_app.list_path("Home/Chores.md");

    let system_under_test = test_app.app();

    assert_eq!(system_under_test.lists().active(), Some(&expected_active));
    assert_eq!(system_under_test.focus(), Focus::Lists);
}

#[rstest]
#[case::down("ctrl-d", "list-10.md")]
#[case::down_twice("pagedown pagedown", "list-20.md")]
#[case::up("G ctrl-u", "list-19.md")]
fn half_page_moves_half_the_standard_pane_height(#[case] keys: &str, #[case] expected: &str) {
    let mut test_app = many_lists(30);
    test_app.press(keys);
    let expected_key = key(&test_app, expected);

    let system_under_test = test_app.app().lists().outline().selected();

    assert_eq!(system_under_test, Some(&expected_key));
}

#[test]
fn pane_shows_folders_lists_and_counters() {
    let test_app = TestApp::new(WORKSPACE);

    let system_under_test = test_app.render(80, 10);

    assert_snapshot!(system_under_test);
}

#[test]
fn pane_marks_the_active_list_while_the_selection_is_elsewhere() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press("G");

    let system_under_test = test_app.render(80, 10);

    assert_snapshot!(system_under_test);
}

#[test]
fn pane_marks_lists_that_cannot_be_loaded() {
    let test_app = TestApp::from_bytes(&[("fine.md", b""), ("other.md", b"- [ ] \xff\n")]);

    let system_under_test = test_app.render(80, 6);

    assert_snapshot!(system_under_test);
    assert_eq!(style_at(&system_under_test, 12, 2), UNREADABLE);
}

#[test]
fn pane_scrolls_to_keep_the_selection_visible() {
    let mut test_app = many_lists(30);
    test_app.press("G");

    let system_under_test = test_app.render(80, 10);

    assert_snapshot!(system_under_test);
}

#[test]
fn empty_workspace_tells_how_to_create_a_list() {
    let test_app = TestApp::new(&[]);

    let system_under_test = test_app.render(80, 6);

    assert_snapshot!(system_under_test);
}

#[test]
fn counters_follow_edits_immediately() {
    let mut test_app = TestApp::new(WORKSPACE);
    let chores = test_app.list_path("Home/Chores.md");
    test_app.run(|app| {
        app.edit_list(&chores, |document| {
            let id = document.tasks().next().unwrap().id();
            document.toggle_done(id, None)
        })
    });
    let expected_row = "   • Chores       1/2 ";

    let system_under_test = test_app.render(80, 10);

    assert_eq!(lists_row(&system_under_test, 2), expected_row);
}

#[test]
fn folders_and_counters_use_their_theme_slots() {
    let test_app = TestApp::new(WORKSPACE);

    let system_under_test = test_app.render(80, 10);

    assert_eq!(style_at(&system_under_test, 4, 1), FOLDER);
    assert_eq!(style_at(&system_under_test, 21, 3), LIST_COUNT);
}

#[rstest]
#[case::lists_focused("", SELECTION)]
#[case::tasks_focused("2", SELECTION_UNFOCUSED)]
fn selection_style_follows_the_focus(#[case] keys: &str, #[case] expected: Style) {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press(keys);

    let system_under_test = test_app.render(80, 10);

    assert_eq!(style_at(&system_under_test, 1, 2), expected);
    assert_eq!(style_at(&system_under_test, 22, 2), expected);
}
