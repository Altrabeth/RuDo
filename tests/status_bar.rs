//! The bottom bar: key hints of the focused pane, pending keys and messages that expire.

#[expect(
    dead_code,
    reason = "each test crate uses only part of the shared harness"
)]
mod common;

use std::time::{Duration, Instant};

use insta::assert_snapshot;
use ratatui::backend::TestBackend;
use ratatui::style::{Color, Modifier, Style};
use rstest::rstest;
use rudo::app::{Severity, StatusMessage};
use rudo::markdown::document::{EditOutcome, TodoDocument};

use crate::common::TestApp;

/// How long an info message stays visible.
const INFO_DURATION: Duration = Duration::from_secs(3);
/// The style of a key in a key hint in the default theme.
const KEY_HINT: Style = Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD);
/// The style of the label of a key hint in the default theme.
const KEY_HINT_LABEL: Style = Style::new().fg(Color::Gray);

fn toggle_first_task(document: &mut TodoDocument) -> EditOutcome {
    let id = document.tasks().next().unwrap().id();
    document.toggle_done(id, None)
}

/// Lets `test_app` see the time `now`, as the runtime does before every draw.
fn tick(test_app: &mut TestApp, now: Instant) {
    test_app.run(|app| {
        app.tick(now);
        Vec::new()
    });
}

/// Shows `message` in the bottom bar of `test_app`.
fn show(test_app: &mut TestApp, message: StatusMessage) {
    test_app.run(|app| {
        app.show_message(message);
        Vec::new()
    });
}

/// Fails a save of `groceries.md` because another program changed it.
fn fail_a_save(test_app: &mut TestApp) {
    let list = test_app.list_path("groceries.md");
    test_app.write("groceries.md", "- [ ] Buy bread\n");
    test_app.run(|app| app.edit_list(&list, toggle_first_task));
}

/// Returns the text of the bottom row.
fn bottom_row(backend: &TestBackend) -> String {
    let buffer = backend.buffer();
    let row = buffer.area.height - 1;
    (0..buffer.area.width)
        .map(|column| buffer[(column, row)].symbol())
        .collect()
}

/// Returns the colour and modifiers of the cell at `x`, `y`; the background is never set.
fn style_at(backend: &TestBackend, x: u16, y: u16) -> Style {
    let cell = &backend.buffer()[(x, y)];
    Style::new().fg(cell.fg).add_modifier(cell.modifier)
}

#[test]
fn lists_pane_shows_its_hints() {
    let mut test_app = TestApp::new(&[]);

    let system_under_test = test_app.render(80, 6);

    assert_snapshot!(system_under_test);
}

#[test]
fn tasks_pane_shows_its_hints() {
    let mut test_app = TestApp::new(&[]);
    test_app.press("2");

    let system_under_test = test_app.render(80, 6);

    assert_snapshot!(system_under_test);
}

#[rstest]
#[case::all_hints(80, " o new task  a subtask  i edit  x done  dd delete  D due date ")]
#[case::some_dropped(50, " o new task  a subtask  i edit  x done ")]
#[case::narrowest(24, " o new task ")]
fn narrow_bar_drops_hints_from_the_right_but_keeps_help(
    #[case] width: u16,
    #[case] expected_hints: &str,
) {
    let mut test_app = TestApp::new(&[]);
    test_app.press("2");
    let expected = format!(
        "{expected_hints:<width$}",
        width = usize::from(width) - " ? help ".len()
    ) + " ? help ";

    let system_under_test = test_app.render(width, 6);

    assert_eq!(bottom_row(&system_under_test), expected);
}

#[test]
fn inline_task_editor_shows_the_key_for_the_next_task() {
    let mut test_app = TestApp::new(&[("groceries.md", "- [ ] Buy milk\n")]);
    test_app.press("2 o");

    let system_under_test = test_app.render(80, 6);

    assert_snapshot!(system_under_test);
}

#[test]
fn other_text_inputs_do_not_show_the_key_for_the_next_task() {
    let mut test_app = TestApp::new(&[]);
    test_app.press("o");
    let expected = format!("{:<80}", " enter accept  esc cancel ");

    let system_under_test = test_app.render(80, 24);

    assert_eq!(bottom_row(&system_under_test), expected);
}

#[test]
fn hints_use_the_key_hint_slots() {
    let mut test_app = TestApp::new(&[]);

    let system_under_test = test_app.render(80, 6);

    assert_eq!(system_under_test.buffer()[(1, 5)].symbol(), "o");
    assert_eq!(style_at(&system_under_test, 1, 5), KEY_HINT);
    assert_eq!(system_under_test.buffer()[(3, 5)].symbol(), "n");
    assert_eq!(style_at(&system_under_test, 3, 5), KEY_HINT_LABEL);
}

#[test]
fn pending_keys_are_shown_at_the_right_end() {
    let mut test_app = TestApp::new(&[]);
    test_app.press("2 z");

    let system_under_test = test_app.render(80, 6);

    assert_snapshot!(system_under_test);
}

#[test]
fn completed_sequence_clears_the_pending_keys() {
    let mut test_app = TestApp::new(&[]);
    test_app.press("2 g g");

    let system_under_test = test_app.render(80, 6);

    assert!(bottom_row(&system_under_test).ends_with(" ? help "));
}

#[test]
fn error_message_replaces_the_hints() {
    let mut test_app = TestApp::new(&[]);
    show(
        &mut test_app,
        StatusMessage::error("cannot save groceries.md"),
    );

    let system_under_test = test_app.render(60, 6);

    assert_snapshot!(system_under_test);
}

#[test]
fn info_message_replaces_the_hints() {
    let mut test_app = TestApp::new(&[]);
    show(&mut test_app, StatusMessage::info("list created"));

    let system_under_test = test_app.render(60, 6);

    assert_snapshot!(system_under_test);
}

#[test]
fn error_message_disappears_at_the_next_key_press() {
    let mut test_app = TestApp::new(&[("groceries.md", "- [ ] Buy milk\n")]);
    fail_a_save(&mut test_app);

    let system_under_test = test_app.press("j");

    assert_eq!(system_under_test.app().status_message(), None);
}

#[test]
fn error_message_does_not_expire_with_time() {
    let mut test_app = TestApp::new(&[("groceries.md", "- [ ] Buy milk\n")]);
    fail_a_save(&mut test_app);
    let shown_at = Instant::now();
    tick(&mut test_app, shown_at);

    let system_under_test = test_app.run(|app| {
        app.tick(shown_at + Duration::from_secs(3600));
        Vec::new()
    });

    assert_eq!(
        system_under_test
            .app()
            .status_message()
            .map(StatusMessage::severity),
        Some(Severity::Error)
    );
    assert_eq!(system_under_test.app().next_deadline(), None);
}

#[rstest]
#[case::just_shown(Duration::ZERO, Some("list created"))]
#[case::just_before_expiry(Duration::from_millis(2999), Some("list created"))]
#[case::at_expiry(INFO_DURATION, None)]
#[case::long_after(INFO_DURATION * 100, None)]
fn info_message_expires_after_three_seconds(
    #[case] elapsed: Duration,
    #[case] expected: Option<&str>,
) {
    let mut test_app = TestApp::new(&[]);
    show(&mut test_app, StatusMessage::info("list created"));
    let shown_at = Instant::now();
    tick(&mut test_app, shown_at);

    let system_under_test = test_app.run(|app| {
        app.tick(shown_at + elapsed);
        Vec::new()
    });

    assert_eq!(
        system_under_test
            .app()
            .status_message()
            .map(StatusMessage::text),
        expected
    );
}

#[test]
fn info_message_asks_to_be_woken_up_when_it_expires() {
    let mut test_app = TestApp::new(&[]);
    show(&mut test_app, StatusMessage::info("list created"));
    let shown_at = Instant::now();

    let system_under_test = test_app.run(|app| {
        app.tick(shown_at);
        Vec::new()
    });

    assert_eq!(
        system_under_test.app().next_deadline(),
        Some(shown_at + INFO_DURATION)
    );
}

#[test]
fn info_message_survives_key_presses() {
    let mut test_app = TestApp::new(&[]);
    show(&mut test_app, StatusMessage::info("list created"));
    tick(&mut test_app, Instant::now());

    let system_under_test = test_app.press("j");

    assert_eq!(
        system_under_test.app().status_message(),
        Some(&StatusMessage::info("list created"))
    );
}

#[test]
fn without_a_message_there_is_no_deadline() {
    let mut test_app = TestApp::new(&[]);
    tick(&mut test_app, Instant::now());

    let system_under_test = test_app.app();

    assert_eq!(system_under_test.next_deadline(), None);
}
