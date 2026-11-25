//! Setting recurrence rules with `r` and creating the next occurrence when a recurring task is
//! completed.

#[expect(
    dead_code,
    reason = "each test crate uses only part of the shared harness"
)]
mod common;

use std::borrow::Cow;

use insta::assert_snapshot;
use rstest::rstest;
use rudo::app::prompt::PromptOverlay;
use rudo::app::{Mode, StatusMessage};
use rudo::markdown::task_line::TaskStatus;
use rudo::ui::popup::Prompt;
use rudo::ui::text_input::TextInput;

use crate::common::TestApp;

const HINT: &str = "every day, every 2 weeks, every month; empty clears";

fn prompt_of(test_app: &TestApp) -> &PromptOverlay {
    match test_app.app().mode() {
        Mode::Prompt(prompt) => prompt,
        mode => panic!("no prompt is open, the mode is {mode:?}"),
    }
}

/// Returns the recurrence prompt holding `text` with `preview` and `error`.
fn recurrence_prompt(text: &str, preview: Option<&'static str>, error: Option<&str>) -> Prompt {
    let mut prompt = Prompt::new("Recurrence", TextInput::with_value(text)).with_hint(HINT);
    prompt.set_preview(preview.map(Cow::Borrowed));
    prompt.set_error(error.map(|error| Cow::Owned(error.to_owned())));
    prompt
}

#[rstest]
#[case::with_a_rule(
    "- [ ] Water plants [repeat:: every week]\n",
    "every week",
    Some("every week"),
    None
)]
#[case::without_a_rule("- [ ] Water plants\n", "", Some("no recurrence"), None)]
#[case::unsupported_rule(
    "- [ ] Water plants [repeat:: every weekday]\n",
    "every weekday",
    None,
    Some("unsupported recurrence rule")
)]
fn recurrence_prompt_holds_the_current_rule(
    #[case] content: &str,
    #[case] expected_text: &str,
    #[case] expected_preview: Option<&'static str>,
    #[case] expected_error: Option<&str>,
) {
    let files = [("Home.md", content)];
    let expected = recurrence_prompt(expected_text, expected_preview, expected_error);
    let mut test_app = TestApp::new(&files);
    test_app.press("tab r");

    let system_under_test = prompt_of(&test_app).prompt();

    assert_eq!(system_under_test, &expected);
}

#[rstest]
#[case::canonical("Every 2 Week", "- [ ] Water plants [repeat:: every 2 weeks]\n")]
#[case::empty_clears("", "- [ ] Water plants\n")]
fn submit_writes_the_rule(#[case] text: &str, #[case] expected: &str) {
    let files = [("Home.md", "- [ ] Water plants [repeat:: every day]\n")];
    let mut test_app = TestApp::new(&files);
    test_app
        .press("tab r ctrl-u")
        .type_text(text)
        .press("enter");

    let system_under_test = &test_app;

    assert_eq!(system_under_test.read("Home.md"), expected);
    assert!(matches!(system_under_test.app().mode(), Mode::Normal));
}

#[test]
fn invalid_rule_keeps_the_prompt_open_and_the_file_unchanged() {
    let content = "- [ ] Water plants\n";
    let files = [("Home.md", content)];
    let expected = recurrence_prompt("every hour", None, Some("unsupported recurrence rule"));
    let mut test_app = TestApp::new(&files);
    test_app
        .press("tab r")
        .type_text("every hour")
        .press("enter");

    let system_under_test = &test_app;

    assert_eq!(system_under_test.read("Home.md"), content);
    assert_eq!(prompt_of(system_under_test).prompt(), &expected);
}

#[test]
fn prompt_shows_the_hint_and_the_stored_form() {
    let files = [("Home.md", "- [ ] Water plants\n")];
    let mut test_app = TestApp::new(&files);
    test_app.press("tab r").type_text("every 1 Month");

    let system_under_test = test_app.render(100, 10);

    assert_snapshot!(system_under_test);
}

#[test]
fn rows_show_the_rule_before_priority_and_due_date() {
    let files = [(
        "Home.md",
        "- [ ] Water plants [due:: 2026-03-15] [priority:: high] [repeat:: every week]\n\
         - [ ] Clean [repeat:: every weekday]\n",
    )];
    let mut test_app = TestApp::new(&files);

    let system_under_test = test_app.render(80, 6);

    assert_snapshot!(system_under_test);
}

#[rstest]
#[case::after_the_due_date(
    "- [ ] Water plants #home [due:: 2026-03-10] [priority:: high] [repeat:: every week]\n",
    "- [ ] Water plants #home [due:: 2026-03-17] [priority:: high] [repeat:: every week]\n\
     - [x] Water plants #home [due:: 2026-03-10] [priority:: high] [repeat:: every week] \
     [completion:: 2026-03-14]\n"
)]
#[case::after_today_without_a_due_date(
    "- [ ] Water plants [repeat:: every 2 days]\n",
    "- [ ] Water plants [repeat:: every 2 days] [due:: 2026-03-16]\n\
     - [x] Water plants [repeat:: every 2 days] [completion:: 2026-03-14]\n"
)]
#[case::without_subtasks(
    "- [ ] Trip [due:: 2026-03-14] [repeat:: every month]\n\t- [ ] Pack\n",
    "- [ ] Trip [due:: 2026-04-14] [repeat:: every month]\n\
     - [x] Trip [due:: 2026-03-14] [repeat:: every month] [completion:: 2026-03-14]\n\
     \t- [ ] Pack\n"
)]
fn completing_a_recurring_task_inserts_the_next_occurrence_above(
    #[case] content: &str,
    #[case] expected: &str,
) {
    let files = [("Home.md", content)];
    let mut test_app = TestApp::new(&files);

    let system_under_test = test_app.press("tab x");

    assert_eq!(system_under_test.read("Home.md"), expected);
}

#[test]
fn selection_stays_on_the_completed_task() {
    let files = [("Home.md", "- [ ] Water plants [repeat:: every week]\n")];
    let mut test_app = TestApp::new(&files);
    test_app.press("tab x");
    let list = test_app.list_path("Home.md");
    let document = test_app.app().workspace().list(&list).unwrap();
    let document = document.content().unwrap().document();
    let completed = document.tasks().nth(1).unwrap();
    let expected = (completed.id(), TaskStatus::Done);

    let system_under_test = test_app.app().task_outline().unwrap().selected();

    assert_eq!(
        system_under_test.map(|&task| (task, completed.task().status())),
        Some(expected)
    );
}

#[test]
fn reopening_the_completed_task_keeps_the_new_occurrence() {
    let files = [("Home.md", "- [ ] Water plants [repeat:: every day]\n")];
    let expected = "- [ ] Water plants [repeat:: every day] [due:: 2026-03-15]\n\
                    - [ ] Water plants [repeat:: every day]\n";
    let mut test_app = TestApp::new(&files);

    let system_under_test = test_app.press("tab x x");

    assert_eq!(system_under_test.read("Home.md"), expected);
}

#[test]
fn one_undo_reverts_the_completion_and_the_new_occurrence() {
    let content = "- [ ] Water plants [repeat:: every day]\n";
    let files = [("Home.md", content)];
    let mut test_app = TestApp::new(&files);

    let system_under_test = test_app.press("tab x u");

    assert_eq!(system_under_test.read("Home.md"), content);
}

#[test]
fn unsupported_rule_completes_the_task_and_tells_why() {
    let files = [("Home.md", "- [ ] Clean [repeat:: every weekday]\n")];
    let expected_file = "- [x] Clean [repeat:: every weekday] [completion:: 2026-03-14]\n";
    let expected_message =
        StatusMessage::error("unsupported recurrence rule: no new occurrence was created");
    let mut test_app = TestApp::new(&files);

    let system_under_test = test_app.press("tab x");

    assert_eq!(system_under_test.read("Home.md"), expected_file);
    assert_eq!(
        system_under_test.app().status_message(),
        Some(&expected_message)
    );
}
