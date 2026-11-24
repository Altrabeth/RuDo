//! Obsidian tags in the tasks pane: recognised like Obsidian and highlighted.

#[expect(
    dead_code,
    reason = "each test crate uses only part of the shared harness"
)]
mod common;

use insta::assert_snapshot;
use ratatui::backend::TestBackend;
use ratatui::style::{Color, Style};

use crate::common::TestApp;

/// One list whose tasks mix tags with text that only looks like tags.
const WORKSPACE: &[(&str, &str)] = &[(
    "Tags.md",
    "- [ ] Buy milk #shopping, #home/kitchen\n- [ ] Learn C# and #123\n\
     - [ ] Run `#code` see [docs](#anchor)\n- [x] Clean #Küche\n- [ ] Plan a trip #travel-plans\n",
)];

/// Styles of the default theme.
const TEXT: Style = Style::new().fg(Color::Reset);
const TAG: Style = Style::new().fg(Color::Magenta);

/// The first column of the text of a row of the tasks pane on an 80 columns wide terminal.
const TEXT_LEFT: u16 = 26;
/// The first column of the text of a row on a terminal too narrow for the lists pane.
const NARROW_TEXT_LEFT: u16 = 2;
/// Columns of a top-level row before its description: fold marker, checkbox and space.
const LEAD_WIDTH: u16 = 6;

/// Returns the colour and modifiers of the cell at `x`, `y`; the background is never set.
fn style_at(backend: &TestBackend, x: u16, y: u16) -> Style {
    let cell = &backend.buffer()[(x, y)];
    Style::new().fg(cell.fg).add_modifier(cell.modifier)
}

#[test]
fn pane_shows_tasks_with_tags() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press("2");

    let system_under_test = test_app.render(80, 8);

    assert_snapshot!(system_under_test);
}

#[test]
fn tags_use_the_tag_slot_and_lookalikes_do_not() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press("2 G");
    let description_left = TEXT_LEFT + LEAD_WIDTH;

    let system_under_test = test_app.render(80, 8);

    assert_eq!(style_at(&system_under_test, description_left, 1), TEXT);
    assert_eq!(style_at(&system_under_test, description_left + 9, 1), TAG);
    assert_eq!(style_at(&system_under_test, description_left + 17, 1), TAG);
    assert_eq!(style_at(&system_under_test, description_left + 18, 1), TEXT);
    assert_eq!(style_at(&system_under_test, description_left + 20, 1), TAG);
    assert_eq!(style_at(&system_under_test, description_left + 7, 2), TEXT);
    assert_eq!(style_at(&system_under_test, description_left + 16, 2), TEXT);
    assert_eq!(style_at(&system_under_test, description_left + 5, 3), TEXT);
    assert_eq!(style_at(&system_under_test, description_left + 24, 3), TEXT);
    assert_eq!(style_at(&system_under_test, description_left + 6, 4), TAG);
}

#[test]
fn cut_tag_keeps_the_tag_slot_up_to_the_ellipsis() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press("2 G");
    let ellipsis_left = NARROW_TEXT_LEFT + LEAD_WIDTH + 25;

    let system_under_test = test_app.render(36, 8);

    assert_snapshot!(system_under_test);
    assert_eq!(style_at(&system_under_test, ellipsis_left - 1, 1), TAG);
    assert_eq!(style_at(&system_under_test, ellipsis_left, 1), TEXT);
}
