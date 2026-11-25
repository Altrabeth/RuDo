//! Setting and clearing due dates with `D`, and showing them colour-coded in the tasks pane.

#[expect(
    dead_code,
    reason = "each test crate uses only part of the shared harness"
)]
mod common;

use std::borrow::Cow;

use insta::assert_snapshot;
use ratatui::backend::TestBackend;
use ratatui::style::{Color, Modifier, Style};
use rstest::rstest;
use rudo::app::Mode;
use rudo::app::prompt::PromptOverlay;
use rudo::ui::popup::Prompt;
use rudo::ui::text_input::TextInput;

use crate::common::TestApp;

/// Today is Saturday, March 14 2026. The tasks in display order: overdue, due today, due
/// tomorrow, due soon, due later, done and overdue, without a due date, with a value that is
/// not a date.
const WORKSPACE: &[(&str, &str)] = &[(
    "Due.md",
    "- [ ] Pay rent [due:: 2026-03-12]\n- [ ] Call mum [due:: 2026-03-14]\n\
     - [ ] Water plants [due:: 2026-03-15]\n- [ ] Buy milk [due:: 2026-03-17]\n\
     - [ ] Plan trip [due:: 2026-04-02]\n- [x] File taxes [due:: 2026-03-01]\n\
     - [ ] Read book\n- [ ] Fix bike [due:: someday]\n",
)];

const HINT: &str = "today, tomorrow, +3d, fri, 2026-12-24; empty clears";

/// Styles of the default theme.
const DUE: Style = Style::new().fg(Color::Blue);
const DUE_SOON: Style = Style::new().fg(Color::Yellow);
const DUE_TODAY: Style = Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD);
const DUE_OVERDUE: Style = Style::new()
    .fg(Color::LightRed)
    .add_modifier(Modifier::BOLD);

/// The last column of the text of a row of the tasks pane on an 80 columns wide terminal.
const TEXT_RIGHT: u16 = 77;

fn prompt_of(test_app: &TestApp) -> &PromptOverlay {
    match test_app.app().mode() {
        Mode::Prompt(prompt) => prompt,
        mode => panic!("no prompt is open, the mode is {mode:?}"),
    }
}

/// Returns the due date prompt holding `text` with `preview` and `error`.
fn due_prompt(text: &str, preview: Option<&'static str>, error: Option<&str>) -> Prompt {
    let mut prompt = Prompt::new("Due date", TextInput::with_value(text)).with_hint(HINT);
    prompt.set_preview(preview.map(Cow::Borrowed));
    prompt.set_error(error.map(|error| Cow::Owned(error.to_owned())));
    prompt
}

/// Returns the colour and modifiers of the cell at `x`, `y`; the background is never set.
fn style_at(backend: &TestBackend, x: u16, y: u16) -> Style {
    let cell = &backend.buffer()[(x, y)];
    Style::new().fg(cell.fg).add_modifier(cell.modifier)
}

#[rstest]
#[case::with_a_due_date(
    "- [ ] Pay rent [due:: 2026-03-20]\n",
    "2026-03-20",
    "Fri 2026-03-20, in 6d"
)]
#[case::without_a_due_date("- [ ] Pay rent\n", "", "no due date")]
fn due_date_prompt_holds_the_current_due_date(
    #[case] content: &str,
    #[case] expected_text: &str,
    #[case] expected_preview: &'static str,
) {
    let files = [("Rent.md", content)];
    let expected = due_prompt(expected_text, Some(expected_preview), None);
    let mut test_app = TestApp::new(&files);
    test_app.press("tab D");

    let system_under_test = prompt_of(&test_app).prompt();

    assert_eq!(system_under_test, &expected);
}

#[rstest]
#[case::weekday("mon", Some("Mon 2026-03-16, in 2d"), None)]
#[case::offset("+3d", Some("Tue 2026-03-17, in 3d"), None)]
#[case::unknown("someday", None, Some("unknown date expression `someday`"))]
#[case::impossible_date("2026-02-30", None, Some("invalid date `2026-02-30`"))]
fn preview_follows_every_edit(
    #[case] text: &str,
    #[case] expected_preview: Option<&'static str>,
    #[case] expected_error: Option<&str>,
) {
    let files = [("Rent.md", "- [ ] Pay rent\n")];
    let expected = due_prompt(text, expected_preview, expected_error);
    let mut test_app = TestApp::new(&files);
    test_app.press("tab D").type_text(text);

    let system_under_test = prompt_of(&test_app).prompt();

    assert_eq!(system_under_test, &expected);
}

#[rstest]
#[case::offset("- [ ] Pay rent\n", "+3d", "- [ ] Pay rent [due:: 2026-03-17]\n")]
#[case::keyword(
    "- [ ] Pay rent [due:: 2026-03-20]\n",
    "today",
    "- [ ] Pay rent [due:: 2026-03-14]\n"
)]
#[case::empty_clears("- [ ] Pay rent [due:: 2026-03-20]\n", "", "- [ ] Pay rent\n")]
#[case::none_clears("- [ ] Pay rent [due:: someday]\n", "none", "- [ ] Pay rent\n")]
fn submit_writes_the_due_date(#[case] content: &str, #[case] text: &str, #[case] expected: &str) {
    let files = [("Rent.md", content)];
    let mut test_app = TestApp::new(&files);
    test_app
        .press("tab D ctrl-u")
        .type_text(text)
        .press("enter");

    let system_under_test = &test_app;

    assert_eq!(system_under_test.read("Rent.md"), expected);
    assert!(matches!(system_under_test.app().mode(), Mode::Normal));
}

#[test]
fn invalid_submit_keeps_the_prompt_open_and_the_file_unchanged() {
    let content = "- [ ] Pay rent [due:: 2026-03-20]\n";
    let files = [("Rent.md", content)];
    let expected = due_prompt(
        "2026-03-20x",
        None,
        Some("unknown date expression `2026-03-20x`"),
    );
    let mut test_app = TestApp::new(&files);
    test_app.press("tab D").type_text("x").press("enter");

    let system_under_test = &test_app;

    assert_eq!(system_under_test.read("Rent.md"), content);
    assert_eq!(prompt_of(system_under_test).prompt(), &expected);
}

#[test]
fn cancel_keeps_the_due_date() {
    let content = "- [ ] Pay rent [due:: 2026-03-20]\n";
    let files = [("Rent.md", content)];
    let mut test_app = TestApp::new(&files);
    test_app.press("tab D ctrl-u esc");

    let system_under_test = &test_app;

    assert_eq!(system_under_test.read("Rent.md"), content);
    assert!(matches!(system_under_test.app().mode(), Mode::Normal));
}

#[test]
fn submit_shows_the_new_due_date_in_the_row() {
    let files = [("Rent.md", "- [ ] Pay rent\n")];
    let mut test_app = TestApp::new(&files);
    test_app.press("tab D").type_text("tom").press("enter");

    let system_under_test = test_app.render(80, 6);

    assert_snapshot!(system_under_test);
}

#[test]
fn prompt_shows_the_hint_and_the_preview() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press("tab j j j D");

    let system_under_test = test_app.render(100, 12);

    assert_snapshot!(system_under_test);
}

#[test]
fn pane_shows_due_dates_relative_to_today() {
    let mut test_app = TestApp::new(WORKSPACE);

    let system_under_test = test_app.render(80, 12);

    assert_snapshot!(system_under_test);
}

#[rstest]
#[case::overdue(1, DUE_OVERDUE)]
#[case::today(2, DUE_TODAY)]
#[case::tomorrow(3, DUE_SOON)]
#[case::soon(4, DUE_SOON)]
#[case::later(5, DUE)]
#[case::done_is_never_overdue(6, DUE)]
#[case::not_a_date(8, DUE)]
fn due_dates_are_styled_by_urgency(#[case] row: u16, #[case] expected: Style) {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press("tab G k");

    let system_under_test = test_app.render(80, 12);

    assert_eq!(style_at(&system_under_test, TEXT_RIGHT, row), expected);
}

#[test]
fn narrow_terminal_cuts_descriptions_before_due_dates() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press("tab");

    let system_under_test = test_app.render(24, 12);

    assert_snapshot!(system_under_test);
}
