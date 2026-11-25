//! Sorting the selected task and its siblings with `s p`, `s d`, `s a` and `s s`.

#[expect(
    dead_code,
    reason = "each test crate uses only part of the shared harness"
)]
mod common;

use rstest::rstest;
use rudo::app::StatusMessage;
use rudo::markdown::document::TaskId;

use crate::common::TestApp;

/// Tasks that every key sorts differently: `Bake bread` has the subtask `Rye flour`.
const GROCERIES: &str = concat!(
    "# Groceries\n",
    "- [x] candles [due:: 2026-12-24]\n",
    "- [ ] Bake bread [priority:: low]\n",
    "\t- [ ] Rye flour\n",
    "- [ ] apples [due:: 2026-11-30] [priority:: high]\n",
    "\n",
    "## Later\n",
    "- [ ] zucchini [priority:: highest]\n",
);

/// Returns the id of the task described as `description` in `Groceries.md`.
fn task_id(test_app: &TestApp, description: &str) -> TaskId {
    let list = test_app.list_path("Groceries.md");
    let document = test_app
        .app()
        .workspace()
        .list(&list)
        .unwrap()
        .content()
        .unwrap()
        .document();
    document
        .tasks()
        .find(|node| node.task().description() == description)
        .unwrap_or_else(|| panic!("Groceries.md has no task {description}"))
        .id()
}

/// Returns the selected task of the active list.
fn selected_task(test_app: &TestApp) -> Option<TaskId> {
    test_app
        .app()
        .task_outline()
        .and_then(|outline| outline.selected().copied())
}

#[rstest]
#[case::priority(
    "2 s p",
    concat!(
        "# Groceries\n",
        "- [ ] apples [due:: 2026-11-30] [priority:: high]\n",
        "- [x] candles [due:: 2026-12-24]\n",
        "- [ ] Bake bread [priority:: low]\n",
        "\t- [ ] Rye flour\n",
    )
)]
#[case::due_date(
    "2 s d",
    concat!(
        "# Groceries\n",
        "- [ ] apples [due:: 2026-11-30] [priority:: high]\n",
        "- [x] candles [due:: 2026-12-24]\n",
        "- [ ] Bake bread [priority:: low]\n",
        "\t- [ ] Rye flour\n",
    )
)]
#[case::alphabetical(
    "2 s a",
    concat!(
        "# Groceries\n",
        "- [ ] apples [due:: 2026-11-30] [priority:: high]\n",
        "- [ ] Bake bread [priority:: low]\n",
        "\t- [ ] Rye flour\n",
        "- [x] candles [due:: 2026-12-24]\n",
    )
)]
#[case::status(
    "2 s s",
    concat!(
        "# Groceries\n",
        "- [ ] Bake bread [priority:: low]\n",
        "\t- [ ] Rye flour\n",
        "- [ ] apples [due:: 2026-11-30] [priority:: high]\n",
        "- [x] candles [due:: 2026-12-24]\n",
    )
)]
fn keys_sort_the_siblings_before_the_heading(#[case] keys: &str, #[case] expected_start: &str) {
    let expected = format!("{expected_start}\n## Later\n- [ ] zucchini [priority:: highest]\n");
    let mut test_app = TestApp::new(&[("Groceries.md", GROCERIES)]);

    let system_under_test = test_app.press(keys);

    assert_eq!(system_under_test.read("Groceries.md"), expected);
}

#[rstest]
#[case::sorted_away_from_the_top("2 s a", "candles")]
#[case::sorted_to_the_top("2 j j j s a", "apples")]
#[case::sorted_with_its_subtasks("2 j s s", "Bake bread")]
fn selection_stays_on_the_sorted_task(#[case] keys: &str, #[case] expected: &str) {
    let mut test_app = TestApp::new(&[("Groceries.md", GROCERIES)]);
    test_app.press(keys);
    let expected = task_id(&test_app, expected);

    let system_under_test = selected_task(&test_app);

    assert_eq!(system_under_test, Some(expected));
}

#[test]
fn undo_restores_the_order_in_one_step() {
    let mut test_app = TestApp::new(&[("Groceries.md", GROCERIES)]);
    test_app.press("2 s a");

    let system_under_test = test_app.press("u");

    assert_eq!(system_under_test.read("Groceries.md"), GROCERIES);
}

#[rstest]
#[case::already_sorted_by_priority("2 G s p")]
#[case::only_subtask("2 j j s a")]
#[case::keys_in_the_lists_pane("s a")]
fn unchanged_order_writes_nothing(#[case] keys: &str) {
    let expected_message = StatusMessage::info("nothing to undo");
    let mut test_app = TestApp::new(&[("Groceries.md", GROCERIES)]);
    test_app.press(keys);

    let system_under_test = test_app.press("2 u");

    assert_eq!(system_under_test.read("Groceries.md"), GROCERIES);
    assert_eq!(
        system_under_test.app().status_message(),
        Some(&expected_message)
    );
}
