//! Raising and lowering the priority of the selected task with `+` and `-`, and the priority
//! markers at the right edge of the task rows.

#[expect(
    dead_code,
    reason = "each test crate uses only part of the shared harness"
)]
mod common;

use insta::assert_snapshot;
use rstest::rstest;

use crate::common::TestApp;

#[rstest]
#[case::normal_to_medium("- [ ] Buy milk\n", "+", "- [ ] Buy milk [priority:: medium]\n")]
#[case::medium_to_highest(
    "- [ ] Buy milk [priority:: medium]\n",
    "+ +",
    "- [ ] Buy milk [priority:: highest]\n"
)]
#[case::low_to_normal("- [ ] Buy milk [priority:: low]\n", "+", "- [ ] Buy milk\n")]
#[case::normal_to_lowest("- [ ] Buy milk\n", "- -", "- [ ] Buy milk [priority:: lowest]\n")]
#[case::medium_to_normal("- [ ] Buy milk [priority:: medium]\n", "-", "- [ ] Buy milk\n")]
#[case::stops_at_highest(
    "- [ ] Buy milk [priority:: high]\n",
    "+ + +",
    "- [ ] Buy milk [priority:: highest]\n"
)]
#[case::stops_at_lowest(
    "- [ ] Buy milk [priority:: low]\n",
    "- - -",
    "- [ ] Buy milk [priority:: lowest]\n"
)]
#[case::unknown_counts_as_normal_up(
    "- [ ] Buy milk [priority:: urgent]\n",
    "+",
    "- [ ] Buy milk [priority:: medium]\n"
)]
#[case::unknown_counts_as_normal_down(
    "- [ ] Buy milk [priority:: urgent]\n",
    "-",
    "- [ ] Buy milk [priority:: low]\n"
)]
#[case::keeps_other_fields(
    "- [ ] Buy milk [due:: 2026-03-20] #shopping\n",
    "+",
    "- [ ] Buy milk #shopping [due:: 2026-03-20] [priority:: medium]\n"
)]
fn keys_step_the_priority_of_the_selected_task(
    #[case] content: &str,
    #[case] keys: &str,
    #[case] expected: &str,
) {
    let files = [("Groceries.md", content)];
    let mut test_app = TestApp::new(&files);
    test_app.press("tab");

    let system_under_test = test_app.press(keys);

    assert_eq!(system_under_test.read("Groceries.md"), expected);
}

#[test]
fn only_the_selected_subtask_changes() {
    let files = [("Groceries.md", "- [ ] Buy milk\n\t- [ ] Oat milk\n")];
    let expected = "- [ ] Buy milk\n\t- [ ] Oat milk [priority:: high]\n";
    let mut test_app = TestApp::new(&files);

    let system_under_test = test_app.press("tab j + +");

    assert_eq!(system_under_test.read("Groceries.md"), expected);
}

#[test]
fn highest_priority_is_not_saved_again() {
    let files = [("Groceries.md", "- [ ] Buy milk [priority::highest]\n")];
    let expected = "- [ ] Buy milk [priority::highest]\n";
    let mut test_app = TestApp::new(&files);

    let system_under_test = test_app.press("tab +");

    assert_eq!(system_under_test.read("Groceries.md"), expected);
}

#[test]
fn pane_shows_the_priority_markers() {
    let files = [(
        "Priorities.md",
        "- [ ] Call the bank [priority:: highest]\n- [ ] Pay rent [priority:: high]\n\
         - [ ] Book flights [priority:: medium]\n- [ ] Water plants\n\
         - [ ] Sort photos [priority:: low]\n- [ ] Learn juggling [priority:: lowest]\n\
         - [x] Fix the bike [priority:: urgent]\n",
    )];
    let mut test_app = TestApp::new(&files);
    test_app.press("2");

    let system_under_test = test_app.render(80, 10);

    assert_snapshot!(system_under_test);
}
