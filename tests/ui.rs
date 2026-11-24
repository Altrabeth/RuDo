//! What the user sees: the rendered interface at different terminal sizes.

#[expect(
    dead_code,
    reason = "each test crate uses only part of the shared harness"
)]
mod common;

use insta::assert_snapshot;
use ratatui::backend::TestBackend;
use ratatui::style::{Color, Modifier, Style};
use rstest::rstest;

use crate::common::TestApp;

/// The style of the focused border in the default theme.
const FOCUSED_BORDER: Style = Style::new().fg(Color::Cyan);
/// The style of a border without focus in the default theme.
const UNFOCUSED_BORDER: Style = Style::new().fg(Color::DarkGray);
/// The style of the focused title in the default theme.
const FOCUSED_TITLE: Style = Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD);
/// The style of a title without focus in the default theme.
const UNFOCUSED_TITLE: Style = Style::new().fg(Color::Gray);

#[test]
fn standard_terminal_shows_both_panes() {
    let mut test_app = TestApp::new(&[]);

    let system_under_test = test_app.render(80, 24);

    assert_snapshot!(system_under_test);
}

#[test]
fn narrow_terminal_shows_the_focused_pane_with_a_hint() {
    let mut test_app = TestApp::new(&[]);
    test_app.press("2");

    let system_under_test = test_app.render(40, 12);

    assert_snapshot!(system_under_test);
}

#[test]
fn tiny_terminal_shows_that_it_is_too_small() {
    let mut test_app = TestApp::new(&[]);

    let system_under_test = test_app.render(20, 4);

    assert_snapshot!(system_under_test);
}

#[test]
fn large_terminal_caps_the_lists_pane_width() {
    let mut test_app = TestApp::new(&[]);

    let system_under_test = test_app.render(200, 50);

    assert_snapshot!(system_under_test);
}

#[rstest]
#[case::lists_focused("", FOCUSED_BORDER, UNFOCUSED_BORDER)]
#[case::tasks_focused("2", UNFOCUSED_BORDER, FOCUSED_BORDER)]
fn focused_pane_has_the_focused_border(
    #[case] keys: &str,
    #[case] expected_lists_border: Style,
    #[case] expected_tasks_border: Style,
) {
    let mut test_app = TestApp::new(&[]);
    test_app.press(keys);

    let system_under_test = test_app.render(80, 24);

    assert_eq!(style_at(&system_under_test, 0, 0), expected_lists_border);
    assert_eq!(style_at(&system_under_test, 23, 22), expected_lists_border);
    assert_eq!(style_at(&system_under_test, 24, 0), expected_tasks_border);
    assert_eq!(style_at(&system_under_test, 79, 22), expected_tasks_border);
}

#[test]
fn focused_pane_title_uses_the_focused_title_style() {
    let mut test_app = TestApp::new(&[]);

    let system_under_test = test_app.render(80, 24);

    assert_eq!(system_under_test.buffer()[(2, 0)].symbol(), "L");
    assert_eq!(style_at(&system_under_test, 2, 0), FOCUSED_TITLE);
    assert_eq!(system_under_test.buffer()[(26, 0)].symbol(), "T");
    assert_eq!(style_at(&system_under_test, 26, 0), UNFOCUSED_TITLE);
}

/// Returns the colour and modifiers of the cell at `x`, `y`; the background is never set.
fn style_at(backend: &TestBackend, x: u16, y: u16) -> Style {
    let cell = &backend.buffer()[(x, y)];
    Style::new().fg(cell.fg).add_modifier(cell.modifier)
}
