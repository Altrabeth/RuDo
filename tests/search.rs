//! Searching and filtering the lists and tasks pane with `/`.

#[expect(
    dead_code,
    reason = "each test crate uses only part of the shared harness"
)]
mod common;

use insta::assert_snapshot;
use ratatui::backend::TestBackend;
use ratatui::style::{Color, Modifier, Style};
use rstest::rstest;
use rudo::app::lists::EntryKey;
use rudo::app::{Mode, StatusMessage};
use rudo::ui::text_input::TextInput;

use crate::common::TestApp;

/// The tasks of `Groceries` in display order: `Buy milk` (`Check lactose-free`, `Oat milk`),
/// `Bake bread` done with the open subtask `Buy flour for bread`, `Clean fridge` (`Throw
/// away old Milk`) and `Call mum`.
const TASKS: &[(&str, &str)] = &[(
    "Groceries.md",
    "# Groceries\n\n- [ ] Buy milk\n\t- [ ] Check lactose-free\n\t- [ ] Oat milk\n\
     - [x] Bake bread\n\t- [ ] Buy flour for bread\n- [ ] Clean fridge\n\
     \t- [ ] Throw away old Milk\n- [ ] Call mum\n",
)];

/// Rows in display order: `Home/` (`Chores`, `Garden`), `Work/` (`Reports`), `Inbox`.
const LISTS: &[(&str, &str)] = &[
    ("Home/Chores.md", "- [ ] Vacuum\n"),
    ("Home/Garden.md", "- [ ] Water plants\n"),
    ("Work/Reports.md", "- [ ] Write report\n"),
    ("Inbox.md", "- [ ] Call mum\n"),
];

/// Styles of the default theme.
const SEARCH_MATCH: Style = Style::new()
    .fg(Color::Yellow)
    .add_modifier(Modifier::REVERSED);
const FILTER_CONTEXT: Style = Style::new().fg(Color::DarkGray);
const TEXT: Style = Style::new().fg(Color::Reset);

/// Returns the descriptions of the tasks the tasks pane shows, in display order.
fn shown_tasks(test_app: &TestApp) -> Vec<String> {
    let app = test_app.app();
    let outline = app.task_outline().unwrap();
    let loaded = app.active_list().unwrap().content().unwrap();
    let items = rudo::app::tasks::visible_task_items(
        loaded.document(),
        app.completed_tasks(),
        app.task_filter(),
    );
    outline
        .visible_rows(&items)
        .iter()
        .map(|row| row.item().task().description().to_owned())
        .collect()
}

/// Returns the description of the selected task of the active list.
fn selected_task(test_app: &TestApp) -> Option<String> {
    let app = test_app.app();
    let task = *app.task_outline()?.selected()?;
    let loaded = app.active_list()?.content().ok()?;
    let node = loaded.document().tasks().find(|node| node.id() == task)?;
    Some(node.task().description().to_owned())
}

/// Returns the names of the folders and lists the lists pane shows, in display order.
fn shown_entries(test_app: &TestApp) -> Vec<String> {
    let app = test_app.app();
    let items = app.lists().items(app.workspace());
    app.lists()
        .outline()
        .visible_rows(&items)
        .iter()
        .filter_map(|row| row.item().entry().map(|entry| entry.name().to_owned()))
        .collect()
}

/// Returns the text of row `y` of a rendering.
fn row(backend: &TestBackend, y: u16) -> String {
    let width = backend.buffer().area.width;
    (0..width)
        .map(|x| backend.buffer()[(x, y)].symbol())
        .collect()
}

/// Returns the column at which `text` starts in `row`, counted in cells.
fn column_of(row: &str, text: &str) -> u16 {
    let byte = row.find(text).unwrap();
    u16::try_from(row[..byte].chars().count()).unwrap()
}

/// Returns the colour and modifiers of the cell at `x`, `y`; the background is never set.
fn style_at(backend: &TestBackend, x: u16, y: u16) -> Style {
    let cell = &backend.buffer()[(x, y)];
    Style::new().fg(cell.fg).add_modifier(cell.modifier)
}

#[test]
fn typing_filters_the_tasks_pane_live() {
    let mut test_app = TestApp::new(TASKS);

    let system_under_test = test_app.press("tab /").type_text("milk").render(80, 12);

    assert_snapshot!(system_under_test);
}

#[test]
fn search_prompt_opens_in_the_bottom_bar_with_a_slash() {
    let mut test_app = TestApp::new(TASKS);
    let expected = " /mi";

    let system_under_test = test_app.press("tab /").type_text("mi").render(80, 12);

    assert!(row(&system_under_test, 11).starts_with(expected));
    assert!(matches!(test_app.app().mode(), Mode::Search(_)));
}

/// Outside the pickers `ctrl-k` is not bound, so it still deletes to the end of the input.
#[test]
fn ctrl_k_deletes_to_the_end_of_the_search_prompt() {
    let mut test_app = TestApp::new(TASKS);
    test_app.press("tab /").type_text("milk").press("left left");
    let expected = " /mi";

    let system_under_test = test_app.press("ctrl-k").render(80, 12);

    assert!(row(&system_under_test, 11).starts_with(expected));
    assert!(!row(&system_under_test, 11).starts_with(" /milk"));
}

#[test]
fn matches_are_shown_with_their_ancestors() {
    let mut test_app = TestApp::new(TASKS);
    let expected = [
        "Buy milk",
        "Oat milk",
        "Clean fridge",
        "Throw away old Milk",
    ];

    let system_under_test = test_app.press("tab /").type_text("milk");

    assert_eq!(shown_tasks(system_under_test), expected);
}

#[rstest]
#[case::lowercase_ignores_case("old milk", &["Clean fridge", "Throw away old Milk"])]
#[case::uppercase_respects_case("Milk", &["Clean fridge", "Throw away old Milk"])]
#[case::uppercase_without_match("MILK", &[])]
fn search_uses_smart_case(#[case] query: &str, #[case] expected: &[&str]) {
    let mut test_app = TestApp::new(TASKS);

    let system_under_test = test_app.press("tab /").type_text(query);

    assert_eq!(shown_tasks(system_under_test), expected);
}

#[test]
fn submit_keeps_the_filter_and_shows_it_in_the_title() {
    let mut test_app = TestApp::new(TASKS);
    let expected_title = "Groceries  1/8  /milk";
    let expected = [
        "Buy milk",
        "Oat milk",
        "Clean fridge",
        "Throw away old Milk",
    ];

    let system_under_test = test_app.press("tab /").type_text("milk").press("enter");

    assert!(row(&system_under_test.render(80, 12), 0).contains(expected_title));
    assert_eq!(shown_tasks(system_under_test), expected);
    assert!(matches!(system_under_test.app().mode(), Mode::Normal));
}

#[test]
fn submitted_filter_leaves_the_keys_to_the_pane() {
    let mut test_app = TestApp::new(TASKS);
    let expected = Some("Oat milk".to_owned());

    let system_under_test = test_app.press("tab /").type_text("milk").press("enter j");

    assert_eq!(selected_task(system_under_test), expected);
}

#[test]
fn cancel_removes_the_filter() {
    let mut test_app = TestApp::new(TASKS);

    let system_under_test = test_app.press("tab /").type_text("milk").press("esc");

    assert_eq!(shown_tasks(system_under_test).len(), 8);
    assert!(!row(&system_under_test.render(80, 12), 0).contains("/milk"));
    assert!(matches!(system_under_test.app().mode(), Mode::Normal));
}

#[test]
fn clear_filter_removes_an_applied_filter() {
    let mut test_app = TestApp::new(TASKS);

    let system_under_test = test_app.press("tab /").type_text("milk").press("enter esc");

    assert_eq!(shown_tasks(system_under_test).len(), 8);
}

#[test]
fn searching_again_continues_the_applied_filter() {
    let mut test_app = TestApp::new(TASKS);
    let expected = Some("milk");

    let system_under_test = test_app
        .press("tab /")
        .type_text("mil")
        .press("enter /")
        .type_text("k");

    assert_eq!(
        system_under_test.app().search_input().map(TextInput::value),
        expected
    );
}

#[test]
fn matched_text_is_highlighted_and_ancestors_are_dimmed() {
    let mut test_app = TestApp::new(TASKS);

    let system_under_test = test_app.press("tab /").type_text("milk").render(80, 12);

    // Row 1 is selected, so row 2 shows the styles without the selection.
    let match_column = column_of(&row(&system_under_test, 2), "milk");
    assert_eq!(style_at(&system_under_test, match_column, 2), SEARCH_MATCH);
    assert_eq!(
        style_at(&system_under_test, match_column + 3, 2),
        SEARCH_MATCH
    );
    assert_eq!(style_at(&system_under_test, match_column - 2, 2), TEXT);
    let context_column = column_of(&row(&system_under_test, 3), "Clean");
    assert_eq!(
        style_at(&system_under_test, context_column, 3),
        FILTER_CONTEXT
    );
}

#[test]
fn folds_are_ignored_while_filtering() {
    let mut test_app = TestApp::new(TASKS);
    let expected = ["Buy milk", "Oat milk"];

    let system_under_test = test_app.press("tab h /").type_text("oat");

    assert_eq!(shown_tasks(system_under_test), expected);
}

#[test]
fn clearing_the_filter_brings_the_folds_back() {
    let mut test_app = TestApp::new(TASKS);
    let expected = [
        "Buy milk",
        "Bake bread",
        "Buy flour for bread",
        "Clean fridge",
        "Throw away old Milk",
        "Call mum",
    ];
    let expected_selection = Some("Clean fridge".to_owned());

    let system_under_test = test_app
        .press("tab h /")
        .type_text("fridge")
        .press("enter esc");

    assert_eq!(shown_tasks(system_under_test), expected);
    assert_eq!(selected_task(system_under_test), expected_selection);
}

#[rstest]
#[case::nearest_before("G", "Throw away old Milk")]
#[case::closer_before("j j j", "Oat milk")]
#[case::shown_selection_stays("j j", "Oat milk")]
fn filtered_out_selection_moves_to_the_nearest_shown_task(
    #[case] keys: &str,
    #[case] expected: &str,
) {
    let mut test_app = TestApp::new(TASKS);
    test_app.press("tab").press(keys);

    let system_under_test = test_app.press("/").type_text("milk");

    assert_eq!(selected_task(system_under_test), Some(expected.to_owned()));
}

#[rstest]
#[case::shown("tab", &["Bake bread", "Buy flour for bread"])]
#[case::below_a_hidden_completed_task("tab H", &[])]
fn completed_tasks_are_hidden_before_filtering(#[case] keys: &str, #[case] expected: &[&str]) {
    let mut test_app = TestApp::new(TASKS);
    test_app.press(keys);

    let system_under_test = test_app.press("/").type_text("flour");

    assert_eq!(shown_tasks(system_under_test), expected);
}

#[test]
fn filter_without_matches_says_so() {
    let mut test_app = TestApp::new(TASKS);

    let system_under_test = test_app.press("tab /").type_text("xyz").render(80, 12);

    assert!(row(&system_under_test, 1).contains("No matches"));
}

#[test]
fn new_task_stays_visible_while_filtered() {
    let mut test_app = TestApp::new(TASKS);
    let expected = [
        "Buy milk",
        "Oat milk",
        "Buy eggs",
        "Clean fridge",
        "Throw away old Milk",
    ];
    let expected_selection = Some("Buy eggs".to_owned());

    let system_under_test = test_app
        .press("tab /")
        .type_text("milk")
        .press("enter o")
        .type_text("Buy eggs")
        .press("enter");

    assert_eq!(shown_tasks(system_under_test), expected);
    assert_eq!(selected_task(system_under_test), expected_selection);
}

#[rstest]
#[case::move_down("J")]
#[case::move_up("K")]
#[case::indent(">")]
#[case::outdent("<")]
fn restructuring_while_filtered_asks_to_clear_the_filter(#[case] keys: &str) {
    let mut test_app = TestApp::new(TASKS);
    let expected_file = test_app.read("Groceries.md");
    let expected_message = Some("press esc to clear the filter before moving tasks");

    let system_under_test = test_app
        .press("tab /")
        .type_text("milk")
        .press("enter j")
        .press(keys);

    assert_eq!(
        system_under_test
            .app()
            .status_message()
            .map(StatusMessage::text),
        expected_message
    );
    assert_eq!(system_under_test.read("Groceries.md"), expected_file);
}

#[test]
fn every_list_has_its_own_filter() {
    let mut test_app = TestApp::new(LISTS);
    let expected = ["Water plants"];

    let system_under_test = test_app
        .press("tab /")
        .type_text("xyz")
        .press("enter 1 j enter");

    assert_eq!(shown_tasks(system_under_test), expected);
}

#[rstest]
#[case::list_with_its_folder("ar", &["Home", "Garden"])]
#[case::lists_in_several_folders("o", &["Home", "Chores", "Work", "Reports", "Inbox"])]
#[case::folders_never_match("home", &[])]
#[case::lowercase_ignores_case("inbox", &["Inbox"])]
#[case::uppercase_respects_case("INBOX", &[])]
fn lists_pane_shows_matching_lists_and_their_folders(
    #[case] query: &str,
    #[case] expected: &[&str],
) {
    let mut test_app = TestApp::new(LISTS);

    let system_under_test = test_app.press("/").type_text(query);

    assert_eq!(shown_entries(system_under_test), expected);
}

#[test]
fn lists_pane_shows_the_filter_in_its_title() {
    let mut test_app = TestApp::new(LISTS);

    let system_under_test = test_app
        .press("/")
        .type_text("ar")
        .press("enter")
        .render(80, 12);

    assert_snapshot!(system_under_test);
}

#[test]
fn lists_filter_dims_folders_and_highlights_names() {
    let mut test_app = TestApp::new(LISTS);

    let system_under_test = test_app.press("/").type_text("ar").render(80, 12);

    let folder_column = column_of(&row(&system_under_test, 1), "Home");
    assert_eq!(
        style_at(&system_under_test, folder_column, 1),
        FILTER_CONTEXT
    );
    let match_column = column_of(&row(&system_under_test, 2), "ar");
    assert_eq!(style_at(&system_under_test, match_column, 2), SEARCH_MATCH);
}

#[test]
fn lists_filter_moves_the_selection_to_the_nearest_list() {
    let mut test_app = TestApp::new(LISTS);
    let expected = EntryKey::List(test_app.list_path("Home/Garden.md"));

    let system_under_test = test_app.press("/").type_text("ar");

    assert_eq!(
        system_under_test.app().lists().outline().selected(),
        Some(&expected)
    );
}

#[test]
fn new_list_stays_visible_while_filtered() {
    let mut test_app = TestApp::new(LISTS);
    let expected = ["Home", "Garden", "Shopping"];

    let system_under_test = test_app
        .press("/")
        .type_text("ar")
        .press("enter o")
        .type_text("Shopping")
        .press("enter");

    assert_eq!(shown_entries(system_under_test), expected);
}

#[test]
fn clear_filter_in_the_lists_pane_shows_every_entry() {
    let mut test_app = TestApp::new(LISTS);

    let system_under_test = test_app.press("/").type_text("ar").press("enter esc");

    assert_eq!(shown_entries(system_under_test).len(), 6);
}

#[test]
fn folded_folder_is_unfolded_while_filtering() {
    let mut test_app = TestApp::new(LISTS);
    let expected = ["Home", "Garden"];

    let system_under_test = test_app.press("h h /").type_text("ar");

    assert_eq!(shown_entries(system_under_test), expected);
}
