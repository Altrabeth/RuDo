//! Query terms such as `due:today`, `#tag` or `is:open` in the search (`/`) of both panes and
//! in go to anything (`ctrl-p`).

#[expect(
    dead_code,
    reason = "each test crate uses only part of the shared harness"
)]
mod common;

use std::fmt::Write;
use std::time::{Duration, Instant};

use insta::assert_snapshot;
use jiff::civil::date;
use rstest::rstest;
use rudo::app::Mode;
use rudo::app::go_to::GoToCandidate;
use rudo::app::picker::{PickerCandidate, PickerPrompt};
use rudo::app::tasks::visible_task_items;

use crate::common::TestApp;

/// Lists in display order: `Home` (opened on start-up), `Later` and `Work`. Today is Saturday,
/// March 14 2026.
const WORKSPACE: &[(&str, &str)] = &[
    (
        "Home.md",
        "- [ ] Pay rent #bills [due:: 2026-03-14]\n\
         - [ ] Garden\n\
         \t- [ ] Water plants [due:: 2026-03-14]\n\
         - [x] Old bill #bills/old [due:: 2026-03-10] [completion:: 2026-03-01]\n\
         - [ ] Call plumber [due:: 2026-03-10] [priority:: high] [created:: 2026-03-01]\n",
    ),
    ("Later.md", "- [ ] Someday\n"),
    (
        "Work.md",
        "- [ ] Write report [due:: 2026-03-14] [priority:: highest]\n- [ ] Email the team\n",
    ),
];

/// Returns the descriptions of the tasks the tasks pane shows, in display order.
fn shown_tasks(test_app: &TestApp) -> Vec<String> {
    let app = test_app.app();
    let outline = app.task_outline().unwrap();
    let loaded = app.active_list().unwrap().content().unwrap();
    let items = visible_task_items(loaded.document(), app.completed_tasks(), app.task_filter());
    outline
        .visible_rows(&items)
        .iter()
        .map(|row| row.item().task().description().to_owned())
        .collect()
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

fn prompt_of(test_app: &TestApp) -> &PickerPrompt<GoToCandidate> {
    match test_app.app().mode() {
        Mode::GoTo(prompt) => prompt,
        mode => panic!("go to anything is not open, the mode is {mode:?}"),
    }
}

/// Returns the label and the text of the results of go to anything, best first.
fn go_to_results(test_app: &TestApp) -> Vec<(Option<&str>, &str)> {
    prompt_of(test_app)
        .picker()
        .results(0)
        .map(|result| (result.candidate().label(), result.candidate().text()))
        .collect()
}

fn search_text(test_app: &TestApp) -> &str {
    test_app.app().search_input().unwrap().value()
}

#[rstest]
#[case::due_today_with_parents("due:today", &["Pay rent #bills", "Garden", "Water plants"])]
#[case::overdue_skips_done("due:overdue", &["Call plumber"])]
#[case::before_today("due:<today", &["Old bill #bills/old", "Call plumber"])]
#[case::without_due_date("due:none", &["Garden"])]
#[case::nested_tag("#bills", &["Pay rent #bills", "Old bill #bills/old"])]
#[case::priority("priority:>=high", &["Call plumber"])]
#[case::created_last_month("created:>-1m", &["Call plumber"])]
#[case::done_a_week_ago("done:<=-1w", &["Old bill #bills/old"])]
#[case::status("is:done", &["Old bill #bills/old"])]
#[case::text_and_term("rent due:today", &["Pay rent #bills"])]
#[case::words_in_any_order("plants water", &["Garden", "Water plants"])]
#[case::unknown_prefix_is_text("foo:bar", &[])]
fn terms_filter_the_tasks_pane(#[case] query: &str, #[case] expected: &[&str]) {
    let mut test_app = TestApp::new(WORKSPACE);

    let system_under_test = test_app.press("tab /").type_text(query);

    assert_eq!(shown_tasks(system_under_test), expected);
}

#[test]
fn invalid_term_matches_nothing_and_shows_the_error() {
    let mut test_app = TestApp::new(WORKSPACE);

    let system_under_test = test_app.press("tab /").type_text("due:blah").render(80, 6);

    assert_snapshot!(system_under_test);
}

#[test]
fn fixing_an_invalid_term_filters_again() {
    let mut test_app = TestApp::new(WORKSPACE);
    let expected = ["Call plumber"];

    let system_under_test = test_app
        .press("tab /")
        .type_text("priority:hig")
        .type_text("h");

    assert_eq!(shown_tasks(system_under_test), expected);
    assert!(system_under_test.app().search_error().is_none());
}

#[rstest]
#[case::due_today("due:today", &["Home", "Work"])]
#[case::term_and_name("due:today wo", &["Work"])]
#[case::completed_tasks_count("is:done", &["Home"])]
#[case::name_only("lat", &["Later"])]
fn terms_filter_the_lists_pane_by_their_tasks(#[case] query: &str, #[case] expected: &[&str]) {
    let mut test_app = TestApp::new(WORKSPACE);

    let system_under_test = test_app.press("/").type_text(query);

    assert_eq!(shown_entries(system_under_test), expected);
}

#[test]
fn lists_pane_shows_the_number_of_matching_tasks() {
    let mut test_app = TestApp::new(WORKSPACE);

    let system_under_test = test_app.press("/").type_text("due:today").render(80, 6);

    assert_snapshot!(system_under_test);
}

#[test]
fn go_to_with_terms_offers_only_matching_tasks() {
    let mut test_app = TestApp::new(WORKSPACE);
    let expected = [
        (Some("task"), "Pay rent #bills"),
        (Some("task"), "Water plants"),
        (Some("task"), "Write report"),
    ];

    let system_under_test = test_app.press("ctrl-p").type_text("due:today");

    assert_eq!(go_to_results(system_under_test), expected);
}

#[test]
fn go_to_ranks_tasks_matching_the_terms_by_the_text() {
    let mut test_app = TestApp::new(WORKSPACE);
    let expected = [(Some("task"), "Write report")];

    let system_under_test = test_app.press("ctrl-p").type_text("is:open rep");

    assert_eq!(go_to_results(system_under_test), expected);
}

#[test]
fn go_to_reports_an_invalid_term() {
    let mut test_app = TestApp::new(WORKSPACE);
    let expected = Some("is: unknown status `later`");

    let system_under_test = test_app.press("ctrl-p").type_text("is:later");

    assert_eq!(prompt_of(system_under_test).error(), expected);
    assert_eq!(prompt_of(system_under_test).picker().result_count(), 0);
}

#[test]
fn tab_completes_a_term_value_and_cycles() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app.press("tab /").type_text("due:to");
    let expected = ["due:today", "due:tomorrow", "due:to"];

    let system_under_test: Vec<String> = (0..expected.len())
        .map(|_| search_text(test_app.press("tab")).to_owned())
        .collect();

    assert_eq!(system_under_test, expected);
}

#[test]
fn completing_lists_the_candidates_after_the_query() {
    let mut test_app = TestApp::new(WORKSPACE);

    let system_under_test = test_app
        .press("tab /")
        .type_text("is:")
        .press("tab")
        .render(80, 6);

    assert_snapshot!(system_under_test);
}

#[test]
fn completed_term_filters_like_a_typed_one() {
    let mut test_app = TestApp::new(WORKSPACE);
    let expected = ["Call plumber"];

    let system_under_test = test_app.press("tab /").type_text("due:ov").press("tab");

    assert_eq!(shown_tasks(system_under_test), expected);
}

#[test]
fn relative_terms_follow_a_new_date() {
    let mut test_app = TestApp::new(WORKSPACE);
    test_app
        .press("tab /")
        .type_text("due:today")
        .press("enter");
    let expected = ["Old bill #bills/old", "Call plumber"];

    let system_under_test = test_app.run(|app| {
        app.set_today(date(2026, 3, 10));
        Vec::new()
    });

    assert_eq!(shown_tasks(system_under_test), expected);
}

/// The number of lists and of tasks in each of them of the generated workspace.
const LISTS: usize = 100;
const TASKS_PER_LIST: usize = 100;
/// Generous bounds for an unoptimized test build on a slow machine, as for go to anything
/// without terms; they catch filtering that parses the query once per task or copies tasks.
const KEYSTROKE_BUDGET: Duration = Duration::from_millis(500);

/// Starts the app on [`LISTS`] lists of [`TASKS_PER_LIST`] tasks, every tenth of them due
/// today, open and tagged.
fn generated_workspace() -> TestApp {
    let files: Vec<(String, String)> = (0..LISTS)
        .map(|list| {
            let mut content = String::new();
            for task in 0..TASKS_PER_LIST {
                let fields = if task % 10 == 0 {
                    "#home [due:: 2026-03-14] [priority:: high]"
                } else {
                    "[due:: 2026-04-01] [created:: 2026-01-01]"
                };
                writeln!(content, "- [ ] Task {task} of list {list} {fields}")
                    .expect("writing to a String never fails");
            }
            (format!("List {list}.md"), content)
        })
        .collect();
    let files: Vec<(&str, &str)> = files
        .iter()
        .map(|(path, content)| (path.as_str(), content.as_str()))
        .collect();
    TestApp::new(&files)
}

/// Types `text` and returns the time of the slowest keystroke.
fn slowest_keystroke(test_app: &mut TestApp, text: &str) -> Duration {
    text.chars()
        .map(|character| {
            let typed = Instant::now();
            test_app.type_text(character.encode_utf8(&mut [0; 4]));
            typed.elapsed()
        })
        .max()
        .unwrap_or_default()
}

#[test]
fn lists_pane_filters_ten_thousand_tasks_by_terms_within_budget() {
    let mut test_app = generated_workspace();
    test_app.press("/");

    let system_under_test = slowest_keystroke(&mut test_app, "is:open #home due:<=tomorrow");

    assert!(
        system_under_test < KEYSTROKE_BUDGET,
        "{system_under_test:?}"
    );
    assert_eq!(shown_entries(&test_app).len(), LISTS);
}

#[test]
fn go_to_ranks_ten_thousand_tasks_with_terms_within_budget() {
    let mut test_app = generated_workspace();
    test_app.press("ctrl-p");
    let expected_results = LISTS * TASKS_PER_LIST / 10;

    let system_under_test = slowest_keystroke(&mut test_app, "priority:high due:today task");

    assert!(
        system_under_test < KEYSTROKE_BUDGET,
        "{system_under_test:?}"
    );
    assert_eq!(
        prompt_of(&test_app).picker().result_count(),
        expected_results
    );
}
