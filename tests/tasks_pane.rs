//! The tasks pane: the task tree of the active list, navigating it and what the pane shows.

#[expect(
    dead_code,
    reason = "each test crate uses only part of the shared harness"
)]
mod common;

use insta::assert_snapshot;
use ratatui::backend::TestBackend;
use ratatui::style::{Color, Modifier, Style};
use rstest::rstest;
use rudo::app::Focus;
use rudo::markdown::document::TaskId;

use crate::common::TestApp;

/// Lists in display order: `Groceries` (opened at start), `Notes` without tasks, `Work`.
///
/// The tasks of `Groceries` in display order: `Buy milk` (`Check lactose-free`, `Oat milk`
/// (`Compare brands`)), `Bake bread`, `Buy candles`, `Clean fridge`.
const WORKSPACE: &[(&str, &str)] = &[
    (
        "Groceries.md",
        "# Groceries\n\n- [ ] Buy milk #shopping [due:: 2026-03-20]\n\t- [x] Check lactose-free\n\
         \t- [ ] Oat milk\n\t\t- [/] Compare brands\n- [ ] Bake bread\n- [-] Buy candles\n\
         - [/] Clean fridge\n",
    ),
    ("Notes.md", "# Notes\n\nNothing to do.\n"),
    ("Work.md", "- [ ] Write report\n- [ ] Send mail\n"),
];

/// Styles of the default theme.
const TEXT: Style = Style::new().fg(Color::Reset);
const TREE_GUIDE: Style = Style::new().fg(Color::DarkGray);
const TASK_DONE: Style = Style::new().fg(Color::DarkGray);
const TASK_IN_PROGRESS: Style = Style::new().fg(Color::Yellow);
const TASK_CANCELLED: Style = Style::new()
    .fg(Color::DarkGray)
    .add_modifier(Modifier::CROSSED_OUT);
const SELECTION: Style = Style::new()
    .fg(Color::Reset)
    .add_modifier(Modifier::REVERSED);
const SELECTION_UNFOCUSED: Style = Style::new()
    .fg(Color::Reset)
    .add_modifier(Modifier::REVERSED.union(Modifier::DIM));

/// The first column of the tasks pane inside its border on an 80 columns wide terminal.
const PANE_LEFT: u16 = 25;

/// Returns the id of the task described as `description` in the list at `path`.
fn task_id(test_app: &TestApp, path: &str, description: &str) -> TaskId {
    let list = test_app.list_path(path);
    let document = test_app
        .app()
        .workspace()
        .list(&list)
        .unwrap()
        .content()
        .unwrap()
        .document();
    document
        .tasks()
        .find(|node| node.task().description() == description)
        .unwrap_or_else(|| panic!("{path} has no task {description}"))
        .id()
}

/// Returns the selected task of the active list.
fn selected_task(test_app: &TestApp) -> Option<TaskId> {
    test_app
        .app()
        .task_outline()
        .and_then(|outline| outline.selected().copied())
}

/// Returns the colour and modifiers of the cell at `x`, `y`; the background is never set.
fn style_at(backend: &TestBackend, x: u16, y: u16) -> Style {
    let cell = &backend.buffer()[(x, y)];
    Style::new().fg(cell.fg).add_modifier(cell.modifier)
}

#[test]
fn start_selects_the_first_task_of_the_opened_list() {
    let test_app = TestApp::new(WORKSPACE);
    let expected = task_id(&test_app, "Groceries.md", "Buy milk #shopping");

    let system_under_test = selected_task(&test_app);

    assert_eq!(system_under_test, Some(expected));
}

#[test]
fn start_without_lists_has_no_task_outline() {
    let test_app = TestApp::new(&[]);

    let system_under_test = test_app.app().task_outline();

    assert!(system_under_test.is_none());
}

#[rstest]
#[case::down("2 j", "Check lactose-free")]
#[case::up("2 j j k", "Check lactose-free")]
#[case::bottom("2 G", "Clean fridge")]
#[case::top("2 G g g", "Buy milk #shopping")]
#[case::half_page_down("2 ctrl-d", "Clean fridge")]
#[case::collapse_a_subtask_selects_its_parent("2 j j j h", "Oat milk")]
#[case::collapse_folds_a_parent("2 h j", "Bake bread")]
#[case::expand_unfolds("2 h l j", "Check lactose-free")]
#[case::toggle_fold("2 z a j", "Bake bread")]
#[case::toggle_fold_twice("2 z a z a j", "Check lactose-free")]
#[case::fold_all("2 z M j", "Bake bread")]
#[case::unfold_all("2 z M z R j j j", "Compare brands")]
#[case::keys_in_the_lists_pane_do_not_move("j j", "Buy milk #shopping")]
fn keys_move_the_selection_and_fold(#[case] keys: &str, #[case] expected: &str) {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press(keys);
    let expected = task_id(&test_app, "Groceries.md", expected);

    let system_under_test = selected_task(&test_app);

    assert_eq!(system_under_test, Some(expected));
}

#[rstest]
#[case::top_level_leaf("2 G h")]
#[case::folded_top_level_task("2 h h")]
#[case::list_without_tasks("j enter h")]
fn collapse_that_cannot_fold_focuses_the_lists_pane(#[case] keys: &str) {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press(keys);

    let system_under_test = test_app.app().focus();

    assert_eq!(system_under_test, Focus::Lists);
}

#[rstest]
#[case::fold("2 h")]
#[case::select_the_parent("2 j h")]
fn collapse_that_folds_or_moves_keeps_the_focus(#[case] keys: &str) {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press(keys);

    let system_under_test = test_app.app().focus();

    assert_eq!(system_under_test, Focus::Tasks);
}

#[test]
fn opening_a_list_for_the_first_time_selects_its_first_task() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press("2 j 1 G enter");
    let expected = task_id(&test_app, "Work.md", "Write report");

    let system_under_test = selected_task(&test_app);

    assert_eq!(system_under_test, Some(expected));
}

#[rstest]
#[case::back_to_the_first_list("2 G 1 G enter j 1 g g enter", "Groceries.md", "Clean fridge")]
#[case::back_to_the_second_list("2 G 1 G enter j 1 g g enter 1 G enter", "Work.md", "Send mail")]
fn each_list_remembers_its_selected_task(
    #[case] keys: &str,
    #[case] list: &str,
    #[case] expected: &str,
) {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press(keys);
    let expected = task_id(&test_app, list, expected);

    let system_under_test = selected_task(&test_app);

    assert_eq!(system_under_test, Some(expected));
}

#[test]
fn selection_stays_on_its_task_when_an_edit_moves_it() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press("2 j j j j");
    let groceries = test_app.list_path("Groceries.md");
    let bake_bread = task_id(&test_app, "Groceries.md", "Bake bread");
    test_app.run(|app| app.edit_list(&groceries, |document| document.move_up(bake_bread)));

    let system_under_test = test_app.render(80, 12);

    assert_eq!(selected_task(&test_app), Some(bake_bread));
    assert_eq!(style_at(&system_under_test, PANE_LEFT + 4, 1), SELECTION);
}

#[test]
fn pane_shows_the_nested_tree_with_all_statuses() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press("2");

    let system_under_test = test_app.render(80, 12);

    assert_snapshot!(system_under_test);
}

#[test]
fn pane_shows_folded_tasks_with_their_marker() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press("2 j j z a");

    let system_under_test = test_app.render(80, 12);

    assert_snapshot!(system_under_test);
}

#[test]
fn pane_without_focus_shows_the_selection_unfocused() {
    let mut test_app = TestApp::new(WORKSPACE);

    let system_under_test = test_app.render(80, 12);

    assert_snapshot!(system_under_test);
    assert_eq!(
        style_at(&system_under_test, PANE_LEFT, 1),
        SELECTION_UNFOCUSED
    );
}

#[test]
fn pane_title_follows_edits() {
    let mut test_app = TestApp::new(WORKSPACE);
    let groceries = test_app.list_path("Groceries.md");
    let buy_milk = task_id(&test_app, "Groceries.md", "Buy milk #shopping");
    test_app.run(|app| app.edit_list(&groceries, |document| document.toggle_done(buy_milk, None)));
    let expected_title = "╭ Groceries  2/7 ";

    let system_under_test = test_app.render(80, 12);

    let top_row: String = (24..41)
        .map(|x| system_under_test.buffer()[(x, 0)].symbol())
        .collect();
    assert_eq!(top_row, expected_title);
}

#[test]
fn list_without_tasks_tells_how_to_add_one() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press("j enter");

    let system_under_test = test_app.render(80, 6);

    assert_snapshot!(system_under_test);
}

#[test]
fn workspace_without_lists_tells_how_to_open_one() {
    let mut test_app = TestApp::new(&[]);
    test_app.press("2");

    let system_under_test = test_app.render(80, 6);

    assert_snapshot!(system_under_test);
}

#[test]
fn unreadable_list_says_why() {
    let mut test_app = TestApp::from_bytes(&[("broken.md", b"- [ ] \xff\n")]);
    test_app.press("2");

    let system_under_test = test_app.render(80, 6);

    assert_snapshot!(system_under_test);
}

#[test]
fn narrow_pane_truncates_descriptions_and_caps_deep_nesting() {
    let mut test_app = TestApp::new(&[(
        "Deep.md",
        "- [ ] Level one\n\t- [ ] Level two\n\t\t- [ ] Level three\n\t\t\t- [ ] Level four\n\
         \t\t\t\t- [ ] Level five\n\t\t\t\t\t- [ ] Level six\n\t\t\t\t\t\t- [ ] Level seven \
         with a description that is far too long\n",
    )]);
    test_app.press("2");

    let system_under_test = test_app.render(24, 10);

    assert_snapshot!(system_under_test);
}

#[test]
fn rows_use_the_slots_of_their_status_and_the_tree_guide() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press("2 G");
    let text_left = PANE_LEFT + 1;

    let system_under_test = test_app.render(80, 12);

    assert_eq!(style_at(&system_under_test, text_left, 1), TREE_GUIDE);
    assert_eq!(style_at(&system_under_test, text_left + 2, 1), TEXT);
    assert_eq!(style_at(&system_under_test, text_left, 2), TREE_GUIDE);
    assert_eq!(style_at(&system_under_test, text_left + 4, 2), TASK_DONE);
    assert_eq!(style_at(&system_under_test, text_left + 8, 2), TASK_DONE);
    assert_eq!(
        style_at(&system_under_test, text_left + 8, 4),
        TASK_IN_PROGRESS
    );
    assert_eq!(
        style_at(&system_under_test, text_left + 2, 6),
        TASK_CANCELLED
    );
    assert_eq!(
        style_at(&system_under_test, text_left + 6, 6),
        TASK_CANCELLED
    );
    assert_eq!(style_at(&system_under_test, PANE_LEFT, 7), SELECTION);
}
