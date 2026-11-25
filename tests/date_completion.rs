//! Completing the words of the due date (`D`) and recurrence (`r`) prompts with `tab`.

#[expect(
    dead_code,
    reason = "each test crate uses only part of the shared harness"
)]
mod common;

use std::borrow::Cow;
use std::path::Path;

use insta::assert_snapshot;
use rstest::rstest;
use rudo::app::Mode;
use rudo::app::prompt::PromptOverlay;
use rudo::config::Config;
use rudo::ui::popup::Prompt;
use rudo::ui::text_input::TextInput;

use crate::common::TestApp;

/// Today is Saturday, March 14 2026.
const FILES: &[(&str, &str)] = &[("Rent.md", "- [ ] Pay rent\n")];

const DUE_HINT: &str = "today, tomorrow, +3d, fri, 2026-12-24; empty clears";
const DUE_CANDIDATES_OF_T: &str = "today, tomorrow, tuesday, thursday";

fn prompt_of(test_app: &TestApp) -> &PromptOverlay {
    match test_app.app().mode() {
        Mode::Prompt(prompt) => prompt,
        mode => panic!("no prompt is open, the mode is {mode:?}"),
    }
}

fn prompt_text(test_app: &TestApp) -> &str {
    prompt_of(test_app).prompt().input().value()
}

/// Returns the due date prompt holding `text` with `hint` and `preview` or `error`.
fn due_prompt(text: &str, hint: &'static str, preview: Result<&'static str, &str>) -> Prompt {
    let mut prompt = Prompt::new("Due date", TextInput::with_value(text)).with_hint(hint);
    match preview {
        Ok(preview) => prompt.set_preview(Some(Cow::Borrowed(preview))),
        Err(error) => prompt.set_error(Some(Cow::Owned(error.to_owned()))),
    }
    prompt
}

#[rstest]
#[case::unique_prefix("fr", "friday")]
#[case::upper_case("SA", "saturday")]
#[case::clearing_word("n", "none")]
#[case::past("y", "yesterday")]
fn tab_completes_the_due_date_word(#[case] typed: &str, #[case] expected: &str) {
    let mut test_app = TestApp::new(FILES);
    test_app.press("tab D").type_text(typed).press("tab");

    let system_under_test = prompt_text(&test_app);

    assert_eq!(system_under_test, expected);
}

#[rstest]
#[case::first(1, "today", Ok("Sat 2026-03-14, today"))]
#[case::second(2, "tomorrow", Ok("Sun 2026-03-15, tomorrow"))]
#[case::third(3, "tuesday", Ok("Tue 2026-03-17, in 3d"))]
#[case::fourth(4, "thursday", Ok("Thu 2026-03-19, in 5d"))]
#[case::back_to_the_typed_text(5, "t", Err("unknown date expression `t`"))]
#[case::around_again(6, "today", Ok("Sat 2026-03-14, today"))]
fn tab_cycles_through_ambiguous_completions_with_hint_and_preview(
    #[case] tabs: usize,
    #[case] expected_text: &str,
    #[case] expected_preview: Result<&'static str, &str>,
) {
    let expected = due_prompt(expected_text, DUE_CANDIDATES_OF_T, expected_preview);
    let mut test_app = TestApp::new(FILES);
    test_app
        .press("tab D")
        .type_text("t")
        .press(&vec!["tab"; tabs].join(" "));

    let system_under_test = prompt_of(&test_app).prompt();

    assert_eq!(system_under_test, &expected);
}

#[test]
fn tab_without_a_match_changes_nothing() {
    let expected = due_prompt("xyz", DUE_HINT, Err("unknown date expression `xyz`"));
    let mut test_app = TestApp::new(FILES);
    test_app.press("tab D").type_text("xyz").press("tab");

    let system_under_test = prompt_of(&test_app).prompt();

    assert_eq!(system_under_test, &expected);
}

#[test]
fn typing_after_completing_shows_the_hint_again() {
    let expected = due_prompt("today ", DUE_HINT, Ok("Sat 2026-03-14, today"));
    let mut test_app = TestApp::new(FILES);
    test_app
        .press("tab D")
        .type_text("t")
        .press("tab")
        .type_text(" ");

    let system_under_test = prompt_of(&test_app).prompt();

    assert_eq!(system_under_test, &expected);
}

#[test]
fn completed_due_date_is_submitted() {
    let mut test_app = TestApp::new(FILES);
    test_app.press("tab D").type_text("fr").press("tab enter");

    let system_under_test = test_app.read("Rent.md");

    assert_eq!(system_under_test, "- [ ] Pay rent [due:: 2026-03-20]\n");
}

#[test]
fn prompt_lists_the_candidates_while_cycling() {
    let mut test_app = TestApp::new(FILES);
    test_app.press("tab D").type_text("t").press("tab tab");

    let system_under_test = test_app.render(100, 12);

    assert_snapshot!(system_under_test);
}

#[rstest]
#[case::every("e", 1, "every")]
#[case::singular_unit_after_every("every m", 1, "every month")]
#[case::units_cycle("every ", 2, "every week")]
#[case::plural_unit_after_an_interval("Every 3 y", 1, "Every 3 years")]
#[case::multi_byte_space_before("every\u{3000}d", 1, "every\u{3000}day")]
#[case::nothing_after_a_complete_rule("every day ", 1, "every day ")]
#[case::no_weekdays("every mo", 1, "every month")]
fn tab_completes_the_word_valid_at_its_position_in_the_rule(
    #[case] typed: &str,
    #[case] tabs: usize,
    #[case] expected: &str,
) {
    let mut test_app = TestApp::new(FILES);
    test_app
        .press("tab r")
        .type_text(typed)
        .press(&vec!["tab"; tabs].join(" "));

    let system_under_test = prompt_text(&test_app);

    assert_eq!(system_under_test, expected);
}

#[test]
fn tab_completes_the_word_before_the_cursor_and_keeps_the_text_after_it() {
    let mut test_app = TestApp::new(FILES);
    test_app
        .press("tab r")
        .type_text("ev 2 weeks")
        .press("home right right tab enter");

    let system_under_test = test_app.read("Rent.md");

    assert_eq!(
        system_under_test,
        "- [ ] Pay rent [repeat:: every 2 weeks]\n"
    );
}

#[test]
fn name_prompt_ignores_tab() {
    let mut test_app = TestApp::new(FILES);
    test_app.press("o").type_text("to").press("tab");

    let system_under_test = prompt_text(&test_app);

    assert_eq!(system_under_test, "to");
}

#[test]
fn task_text_ignores_tab() {
    let mut test_app = TestApp::new(FILES);
    test_app.press("tab i").type_text(" to").press("tab enter");

    let system_under_test = test_app.read("Rent.md");

    assert_eq!(system_under_test, "- [ ] Pay rent to\n");
}

#[test]
fn search_ignores_tab() {
    let mut test_app = TestApp::new(FILES);
    test_app.press("tab /").type_text("to").press("tab");

    let system_under_test = test_app.render(80, 6);

    assert_snapshot!(system_under_test);
}

#[test]
fn completion_follows_a_rebound_key() {
    let config = Config::from_yaml(
        "keybindings:\n  complete: ctrl-t\n",
        Path::new("config.yaml"),
    )
    .unwrap();
    let mut test_app = TestApp::with_config(FILES, config);
    test_app.press("tab D").type_text("fr").press("tab");
    let after_tab = prompt_text(&test_app).to_owned();
    test_app.press("ctrl-t");

    let system_under_test = prompt_text(&test_app);

    assert_eq!(after_tab, "fr");
    assert_eq!(system_under_test, "friday");
}
