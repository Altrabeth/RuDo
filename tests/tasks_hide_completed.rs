//! Showing and hiding completed tasks with `H`.

#[expect(
    dead_code,
    reason = "each test crate uses only part of the shared harness"
)]
mod common;

use std::path::Path;

use insta::assert_snapshot;
use rudo::config::Config;

use crate::common::TestApp;

/// The tasks of `Groceries` in display order: `Buy milk` (`Check lactose-free`, `Oat milk`),
/// `Bake bread` done with the open subtask `Buy flour`, `Buy candles` cancelled and
/// `Clean fridge` in progress.
const WORKSPACE: &[(&str, &str)] = &[(
    "Groceries.md",
    "# Groceries\n\n- [ ] Buy milk\n\t- [x] Check lactose-free\n\t- [ ] Oat milk\n\
     - [x] Bake bread\n\t- [ ] Buy flour\n- [-] Buy candles\n- [/] Clean fridge\n",
)];

/// Returns the description of the selected task of the active list.
fn selected_description(test_app: &TestApp) -> Option<String> {
    let app = test_app.app();
    let task = *app.task_outline()?.selected()?;
    let loaded = app.active_list()?.content().ok()?;
    let node = loaded.document().tasks().find(|node| node.id() == task)?;
    Some(node.task().description().to_owned())
}

/// Returns the top row of a rendering 80 columns wide, which holds the pane titles.
fn top_row(test_app: &mut TestApp) -> String {
    let backend = test_app.render(80, 12);
    (0..80).map(|x| backend.buffer()[(x, 0)].symbol()).collect()
}

#[test]
fn hidden_completed_tasks_leave_out_their_subtasks() {
    let mut test_app = TestApp::new(WORKSPACE);

    let system_under_test = test_app.press("tab H").render(80, 12);

    assert_snapshot!(system_under_test);
}

#[test]
fn pressing_twice_shows_completed_tasks_again() {
    let mut test_app = TestApp::new(WORKSPACE);
    let expected = "[-] Buy candles";

    let system_under_test = test_app.press("tab H H").render(80, 12).to_string();

    assert!(system_under_test.contains(expected));
}

#[test]
fn show_completed_false_hides_them_on_start_up() {
    let config =
        Config::from_yaml("tasks:\n  show_completed: false\n", Path::new("c.yaml")).unwrap();
    let mut test_app = TestApp::with_config(WORKSPACE, config);

    let system_under_test = test_app.render(80, 12).to_string();

    assert!(!system_under_test.contains("Bake bread"));
    assert!(system_under_test.contains("completed hidden"));
}

#[test]
fn title_says_completed_hidden_and_counts_every_task() {
    let mut test_app = TestApp::new(WORKSPACE);
    let expected_title = "Groceries  2/7  completed hidden";

    let system_under_test = top_row(test_app.press("2 H"));

    assert!(system_under_test.contains(expected_title));
}

#[test]
fn title_has_no_note_while_completed_tasks_are_shown() {
    let mut test_app = TestApp::new(WORKSPACE);

    let system_under_test = top_row(&mut test_app);

    assert!(!system_under_test.contains("completed hidden"));
}

#[test]
fn completing_the_selected_task_selects_the_next_visible_task() {
    let mut test_app = TestApp::new(WORKSPACE);
    let expected = Some("Clean fridge".to_owned());

    let system_under_test = test_app.press("tab H x");

    assert_eq!(selected_description(system_under_test), expected);
}

#[test]
fn hiding_a_selected_completed_task_selects_the_next_visible_task() {
    let mut test_app = TestApp::new(WORKSPACE);
    let expected = Some("Oat milk".to_owned());

    let system_under_test = test_app.press("tab j H");

    assert_eq!(selected_description(system_under_test), expected);
}

#[test]
fn showing_completed_tasks_keeps_the_selection() {
    let mut test_app = TestApp::new(WORKSPACE);
    let expected = Some("Clean fridge".to_owned());

    let system_under_test = test_app.press("tab H j j H");

    assert_eq!(selected_description(system_under_test), expected);
}
