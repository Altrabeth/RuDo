//! The help screen: opening and closing it, scrolling through it and filtering it.

#[expect(
    dead_code,
    reason = "each test crate uses only part of the shared harness"
)]
mod common;

use insta::assert_snapshot;
use ratatui::backend::TestBackend;
use ratatui::style::{Color, Style};
use rstest::rstest;
use rudo::app::help::{HelpFilter, HelpOverlay};
use rudo::app::{App, Mode};
use rudo::config::keymap::action::Context;
use rudo::config::theme::{Slot, Theme};
use rudo::ui::help::{HelpHeading, HelpRow, HelpRowKey, HelpSection, filter, help_sections, rows};
use rudo::ui::text_input::TextInput;

use crate::common::TestApp;

fn help_of(app: &App) -> &HelpOverlay {
    let Mode::Help(help) = app.mode() else {
        panic!("the help screen is not open");
    };
    help
}

/// Returns the sections of the help screen of `test_app`, matching `query`.
fn sections_matching(test_app: &TestApp, query: &str) -> Vec<HelpSection> {
    filter(&help_sections(test_app.app().keymap()), query)
}

/// Returns the keys of every row of the full help screen of `test_app`.
fn all_row_keys(test_app: &TestApp) -> Vec<HelpRowKey> {
    rows(&sections_matching(test_app, ""))
        .map(HelpRow::key)
        .collect()
}

/// Returns the text of every row of the rendered terminal.
fn screen_lines(backend: &TestBackend) -> Vec<String> {
    let area = backend.buffer().area;
    (area.top()..area.bottom())
        .map(|y| {
            (area.left()..area.right())
                .map(|x| backend.buffer()[(x, y)].symbol())
                .collect()
        })
        .collect()
}

/// Returns the colour and modifiers of the cell at `x`, `y`; the background is never set.
fn style_at(backend: &TestBackend, x: u16, y: u16) -> Style {
    let cell = &backend.buffer()[(x, y)];
    Style::new().fg(cell.fg).add_modifier(cell.modifier)
}

#[rstest]
#[case::from_the_lists_pane("?")]
#[case::from_the_tasks_pane("2 ?")]
fn help_key_opens_the_help_screen(#[case] keys: &str) {
    let mut test_app = TestApp::new(&[]);

    let system_under_test = test_app.press(keys);

    assert!(matches!(system_under_test.app().mode(), Mode::Help(_)));
    assert_eq!(system_under_test.app().context(), Context::Help);
}

#[rstest]
#[case::question_mark("?")]
#[case::q("q")]
#[case::esc("esc")]
fn close_help_keys_close_the_help_screen(#[case] key: &str) {
    let mut test_app = TestApp::new(&[]);
    test_app.press("?");

    let system_under_test = test_app.press(key);

    assert!(matches!(system_under_test.app().mode(), Mode::Normal));
    assert!(!system_under_test.app().should_quit());
}

#[rstest]
#[case::lists_pane("?", HelpHeading::ListsPane)]
#[case::tasks_pane("2 ?", HelpHeading::TasksPane)]
fn help_opens_at_the_section_of_the_focused_pane(
    #[case] keys: &str,
    #[case] expected: HelpHeading,
) {
    let mut test_app = TestApp::new(&[]);

    let system_under_test = test_app.press(keys);

    let help = help_of(system_under_test.app());
    assert_eq!(help.selected(), Some(HelpRowKey::Heading(expected)));
}

#[rstest]
#[case::down("j", 1)]
#[case::down_arrow("down", 1)]
#[case::up("k", -1)]
#[case::half_page_down("ctrl-d", 10)]
#[case::half_page_up("ctrl-u", -10)]
fn navigation_moves_from_the_opened_section(#[case] key: &str, #[case] rows_moved: isize) {
    let mut test_app = TestApp::new(&[]);
    test_app.press("?");
    let row_keys = all_row_keys(&test_app);
    let section_row = row_keys
        .iter()
        .position(|key| *key == HelpRowKey::Heading(HelpHeading::ListsPane))
        .unwrap();
    let expected = row_keys[section_row.checked_add_signed(rows_moved).unwrap()];

    let system_under_test = test_app.press(key);

    assert_eq!(help_of(system_under_test.app()).selected(), Some(expected));
}

#[rstest]
#[case::top("g g", 0)]
#[case::bottom("G", usize::MAX)]
fn navigation_reaches_the_ends(#[case] keys: &str, #[case] row: usize) {
    let mut test_app = TestApp::new(&[]);
    test_app.press("?");
    let row_keys = all_row_keys(&test_app);
    let expected = row_keys[row.min(row_keys.len() - 1)];

    let system_under_test = test_app.press(keys);

    assert_eq!(help_of(system_under_test.app()).selected(), Some(expected));
}

#[test]
fn search_opens_the_filter_input() {
    let mut test_app = TestApp::new(&[]);
    test_app.press("?");

    let system_under_test = test_app.press("/");

    let help = help_of(system_under_test.app());
    assert_eq!(help.filter(), &HelpFilter::Editing(TextInput::new()));
    assert_eq!(system_under_test.app().context(), Context::Input);
}

#[test]
fn typing_filters_live() {
    let mut test_app = TestApp::new(&[]);
    test_app.press("? /");
    let expected = sections_matching(&test_app, "due");

    let system_under_test = test_app.type_text("due");

    let help = help_of(system_under_test.app());
    assert_eq!(help.sections(), expected);
    assert_eq!(
        help.selected(),
        Some(HelpRowKey::Heading(expected[0].heading()))
    );
}

#[test]
fn editing_keys_change_the_filter() {
    let mut test_app = TestApp::new(&[]);
    test_app.press("? /").type_text("dux");
    let expected = sections_matching(&test_app, "du");

    let system_under_test = test_app.press("backspace");

    assert_eq!(help_of(system_under_test.app()).sections(), expected);
}

#[test]
fn help_key_is_typed_into_the_filter() {
    let mut test_app = TestApp::new(&[]);
    test_app.press("? /");
    let expected = HelpFilter::Editing(TextInput::with_value("?"));

    let system_under_test = test_app.type_text("?");

    assert_eq!(help_of(system_under_test.app()).filter(), &expected);
}

#[test]
fn esc_in_the_filter_clears_it() {
    let mut test_app = TestApp::new(&[]);
    test_app.press("? /").type_text("due");
    let expected = sections_matching(&test_app, "");

    let system_under_test = test_app.press("esc");

    let help = help_of(system_under_test.app());
    assert_eq!(help.filter(), &HelpFilter::Off);
    assert_eq!(help.sections(), expected);
    assert_eq!(system_under_test.app().context(), Context::Help);
}

#[test]
fn enter_in_the_filter_keeps_it_and_navigates_again() {
    let mut test_app = TestApp::new(&[]);
    test_app.press("? /").type_text("due");
    let expected_sections = sections_matching(&test_app, "due");
    let expected_filter = HelpFilter::Applied(TextInput::with_value("due"));

    let system_under_test = test_app.press("enter");

    let help = help_of(system_under_test.app());
    assert_eq!(help.filter(), &expected_filter);
    assert_eq!(help.sections(), expected_sections);
    assert_eq!(system_under_test.app().context(), Context::Help);
}

#[test]
fn pasting_into_the_filter_filters() {
    let mut test_app = TestApp::new(&[]);
    test_app.press("? /");
    let expected = sections_matching(&test_app, "due");

    let system_under_test = test_app.run(|app| app.handle_paste("due"));

    assert_eq!(help_of(system_under_test.app()).sections(), expected);
}

#[test]
fn close_help_after_filtering_closes_the_help_screen() {
    let mut test_app = TestApp::new(&[]);
    test_app.press("? /").type_text("due").press("enter");

    let system_under_test = test_app.press("esc");

    assert!(matches!(system_under_test.app().mode(), Mode::Normal));
}

/// The help screen of dooit could not be scrolled, so entries below the fold were never seen.
#[test]
fn every_entry_can_be_reached_by_scrolling_in_a_small_terminal() {
    let mut test_app = TestApp::new(&[]);
    test_app.resize(80, 10).press("? g g");
    let sections = sections_matching(&test_app, "");
    let row_count = rows(&sections).count();
    let mut seen_lines = screen_lines(&test_app.render(80, 10));
    for _ in 1..row_count {
        seen_lines.extend(screen_lines(&test_app.press("j").render(80, 10)));
    }

    let system_under_test = seen_lines;

    for row in rows(&sections) {
        let found = match row {
            HelpRow::Heading(heading) => {
                let title = heading.to_string();
                system_under_test.iter().any(|line| line.contains(&title))
            }
            HelpRow::Entry(entry) => {
                let description_start: String = entry.description().chars().take(20).collect();
                system_under_test.iter().any(|line| {
                    line.contains(&format!(" {} ", entry.keys()))
                        && line.contains(&description_start)
                })
            }
        };
        assert!(found, "{row:?} was never shown");
    }
}

#[test]
fn standard_terminal_shows_a_scrollbar_and_the_position() {
    let mut test_app = TestApp::new(&[]);
    test_app.press("?");

    let system_under_test = test_app.render(80, 24);

    assert_snapshot!(system_under_test);
}

#[test]
fn large_terminal_shows_the_help_screen() {
    let mut test_app = TestApp::new(&[]);
    test_app.resize(120, 60).press("?");

    let system_under_test = test_app.render(120, 60);

    assert_snapshot!(system_under_test);
}

#[test]
fn move_to_bottom_shows_the_last_rows() {
    let mut test_app = TestApp::new(&[]);
    test_app.press("? G");

    let system_under_test = test_app.render(80, 24);

    assert_snapshot!(system_under_test);
}

#[test]
fn filter_shows_matching_rows_and_the_input() {
    let mut test_app = TestApp::new(&[]);
    test_app.press("? /").type_text("due");

    let system_under_test = test_app.render(80, 24);

    assert_snapshot!(system_under_test);
}

#[test]
fn filter_without_matches_says_so() {
    let mut test_app = TestApp::new(&[]);
    test_app.press("? /").type_text("no such thing");

    let system_under_test = test_app.render(80, 24);

    assert_snapshot!(system_under_test);
}

#[test]
fn tiny_terminal_does_not_break_the_help_screen() {
    let mut test_app = TestApp::new(&[]);
    test_app.resize(20, 4).press("?");

    let system_under_test = test_app.render(20, 4);

    assert_snapshot!(system_under_test);
}

/// Places of the default 80x24 layout after `? g g j`: the second row is selected.
#[rstest]
#[case::border(4, 1, Slot::PopupBorder)]
#[case::heading(5, 2, Slot::HelpHeading)]
#[case::selected_row(5, 3, Slot::Selection)]
#[case::key(7, 4, Slot::HelpKey)]
#[case::description(25, 4, Slot::HelpDescription)]
fn help_screen_takes_its_styles_from_the_theme(#[case] x: u16, #[case] y: u16, #[case] slot: Slot) {
    let mut test_app = TestApp::new(&[]);
    test_app.press("? g g j");
    let theme_style = Theme::default().style(slot);
    let expected = Style::new()
        .fg(theme_style.fg.unwrap_or(Color::Reset))
        .add_modifier(theme_style.add_modifier);

    let system_under_test = test_app.render(80, 24);

    assert_eq!(style_at(&system_under_test, x, y), expected);
}
