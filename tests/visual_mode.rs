//! Visual mode: selecting several tasks or lists with `v` and deleting, yanking or moving them
//! in one step.

#[expect(
    dead_code,
    reason = "each test crate uses only part of the shared harness"
)]
mod common;

use insta::assert_snapshot;
use ratatui::backend::TestBackend;
use ratatui::style::Modifier;
use rstest::rstest;
use rudo::app::confirm::ConfirmOverlay;
use rudo::app::folder_picker::FolderPicker;
use rudo::app::{Mode, StatusMessage};

use crate::common::TestApp;

/// The tasks in display order: `Buy milk`, `Call mum` (`Ask about Sunday`), `Buy bread`,
/// `Water plants`.
const INBOX: &str = "- [ ] Buy milk\n- [ ] Call mum\n\t- [ ] Ask about Sunday\n- [ ] Buy bread\n\
                     - [ ] Water plants\n";

/// The second list, opened from the lists pane with `1 j enter`.
const WORK: &str = "- [ ] Write report\n";

/// Lists in display order: `Home/` (`Chores`), `Inbox`, `Work`; `Home/Chores` is selected and
/// active at the start.
const LISTS: &[(&str, &str)] = &[
    ("Home/Chores.md", "- [ ] Vacuum\n"),
    ("Inbox.md", "- [ ] Call mum\n"),
    ("Work.md", "- [ ] Write report\n- [ ] Plan week\n"),
];

fn tasks_app() -> TestApp {
    TestApp::new(&[("Inbox.md", INBOX), ("Work.md", WORK)])
}

fn confirm_of(test_app: &TestApp) -> &ConfirmOverlay {
    match test_app.app().mode() {
        Mode::Confirm(confirm) => confirm,
        mode => panic!("no confirmation dialog is open, the mode is {mode:?}"),
    }
}

fn folder_picker_of(test_app: &TestApp) -> &FolderPicker {
    match test_app.app().mode() {
        Mode::FolderPicker(folder_picker) => folder_picker,
        mode => panic!("no folder picker is open, the mode is {mode:?}"),
    }
}

fn message_of(test_app: &TestApp) -> Option<&str> {
    test_app.app().status_message().map(StatusMessage::text)
}

fn exists(test_app: &TestApp, path: &str) -> bool {
    test_app.app().workspace().root().join(path).exists()
}

/// Returns the text of the bottom row.
fn bottom_row(backend: &TestBackend) -> String {
    let buffer = backend.buffer();
    let row = buffer.area.height - 1;
    (0..buffer.area.width)
        .map(|column| buffer[(column, row)].symbol())
        .collect()
}

#[rstest]
#[case::two_rows("2 v j", Some(2))]
#[case::to_the_bottom("2 v G", Some(5))]
#[case::back_over_the_anchor("2 j j v j k k", Some(2))]
#[case::left_with_v("2 v j v", None)]
#[case::left_with_esc("2 v j esc", None)]
fn navigation_extends_the_selection(#[case] keys: &str, #[case] expected: Option<usize>) {
    let mut test_app = tasks_app();
    test_app.press(keys);

    let system_under_test = test_app.app().visual_selection_count();

    assert_eq!(system_under_test, expected);
}

#[test]
fn leaving_visual_mode_changes_nothing() {
    let mut test_app = tasks_app();
    test_app.press("2 v j j esc");

    let system_under_test = test_app.read("Inbox.md");

    assert_eq!(system_under_test, INBOX);
    assert!(matches!(test_app.app().mode(), Mode::Normal));
}

#[test]
fn bottom_bar_shows_the_mode_the_count_and_the_keys() {
    let mut test_app = tasks_app();
    test_app.press("2 v j");

    let system_under_test = test_app.render(80, 12);

    assert_eq!(
        bottom_row(&system_under_test).trim_end(),
        " -- VISUAL -- 2 selected  d delete  y yank  > indent  < outdent  esc cancel"
    );
}

#[test]
fn selected_rows_are_drawn_with_the_visual_selection_slot() {
    let mut test_app = tasks_app();
    test_app.press("2 v j j");

    let system_under_test = test_app.render(80, 12);

    assert_snapshot!(system_under_test);
    let buffer = system_under_test.buffer();
    let anchor_row = 1;
    let unselected_row = 5;
    let column = 40;
    assert!(
        buffer[(column, anchor_row)]
            .modifier
            .contains(Modifier::REVERSED)
    );
    assert!(
        !buffer[(column, unselected_row)]
            .modifier
            .contains(Modifier::REVERSED)
    );
}

#[test]
fn deleting_several_tasks_asks_first_and_counts_the_subtasks() {
    let mut test_app = tasks_app();
    test_app.press("2 v j d");

    let system_under_test = confirm_of(&test_app);

    assert_eq!(
        system_under_test.message(),
        "Delete 3 tasks (including subtasks)?"
    );
    assert_eq!(test_app.read("Inbox.md"), INBOX);
}

#[rstest]
#[case::confirmed(
    "y",
    "- [ ] Buy bread\n- [ ] Water plants\n",
    Some("deleted 3 tasks (u to undo)")
)]
#[case::cancelled("n", INBOX, None)]
fn confirming_the_dialog_deletes_the_selection(
    #[case] answer: &str,
    #[case] expected: &str,
    #[case] expected_message: Option<&str>,
) {
    let mut test_app = tasks_app();
    test_app.press("2 v j d").press(answer);

    let system_under_test = test_app.read("Inbox.md");

    assert_eq!(system_under_test, expected);
    assert_eq!(message_of(&test_app), expected_message);
    assert!(matches!(test_app.app().mode(), Mode::Normal));
}

#[test]
fn deleting_a_single_task_does_not_ask() {
    let mut test_app = tasks_app();
    test_app.press("2 G v d");
    let expected = "- [ ] Buy milk\n- [ ] Call mum\n\t- [ ] Ask about Sunday\n- [ ] Buy bread\n";

    let system_under_test = test_app.read("Inbox.md");

    assert_eq!(system_under_test, expected);
    assert!(matches!(test_app.app().mode(), Mode::Normal));
}

#[test]
fn selection_across_a_folded_task_takes_its_hidden_subtasks() {
    let mut test_app = tasks_app();
    test_app.press("2 j z a k v j j d");
    let expected_question = "Delete 4 tasks (including subtasks)?";
    let expected = "- [ ] Water plants\n";

    let system_under_test = confirm_of(&test_app).message().to_owned();

    assert_eq!(system_under_test, expected_question);
    test_app.press("y");
    assert_eq!(test_app.read("Inbox.md"), expected);
}

#[test]
fn selection_while_filtered_takes_only_the_shown_matches() {
    let mut test_app = tasks_app();
    test_app.press("2 /").type_text("buy").press("enter v j d");
    let expected_question = "Delete 2 tasks (including subtasks)?";
    let expected = "- [ ] Call mum\n\t- [ ] Ask about Sunday\n- [ ] Water plants\n";

    let system_under_test = confirm_of(&test_app).message().to_owned();

    assert_eq!(system_under_test, expected_question);
    test_app.press("y");
    assert_eq!(test_app.read("Inbox.md"), expected);
}

#[test]
fn deleted_selection_pasted_into_another_list_moves_the_tasks() {
    let mut test_app = tasks_app();
    test_app.press("2 v j d y 1 j enter p");
    let expected_inbox = "- [ ] Buy bread\n- [ ] Water plants\n";
    let expected_work =
        "- [ ] Write report\n- [ ] Buy milk\n- [ ] Call mum\n\t- [ ] Ask about Sunday\n";

    let system_under_test = (test_app.read("Inbox.md"), test_app.read("Work.md"));

    assert_eq!(
        system_under_test,
        (expected_inbox.to_owned(), expected_work.to_owned())
    );
}

#[test]
fn yanked_selection_is_pasted_in_order() {
    let mut test_app = tasks_app();
    test_app.press("2 j v j y G p");
    let expected = "- [ ] Buy milk\n- [ ] Call mum\n\t- [ ] Ask about Sunday\n- [ ] Buy bread\n\
                    - [ ] Water plants\n- [ ] Call mum\n\t- [ ] Ask about Sunday\n";

    let system_under_test = test_app.read("Inbox.md");

    assert_eq!(system_under_test, expected);
    assert!(matches!(test_app.app().mode(), Mode::Normal));
}

#[test]
fn yank_reports_the_number_of_tasks() {
    let mut test_app = tasks_app();
    test_app.press("2 j v j y");

    let system_under_test = message_of(&test_app);

    assert_eq!(system_under_test, Some("yanked 2 tasks"));
}

#[test]
fn undo_brings_back_a_bulk_delete_in_one_step() {
    let mut test_app = tasks_app();
    test_app.press("2 v G d y u");

    let system_under_test = test_app.read("Inbox.md");

    assert_eq!(system_under_test, INBOX);
}

#[test]
fn reload_that_removes_the_anchor_ends_the_selection() {
    let mut test_app = tasks_app();
    test_app.press("2 v j");
    test_app.write("Inbox.md", "- [ ] Something else\n");
    let list = test_app.list_path("Inbox.md");
    test_app.run(|app| {
        app.reload_list(&list);
        Vec::new()
    });

    let system_under_test = test_app.app().mode();

    assert!(matches!(system_under_test, Mode::Normal));
    assert_eq!(message_of(&test_app), Some("the selection is gone"));
}

#[test]
fn deleting_several_lists_and_a_folder_asks_with_counts() {
    let mut test_app = TestApp::new(&[
        ("Inbox.md", "- [ ] Call mum\n"),
        ("Work.md", "- [ ] Write report\n- [ ] Plan week\n"),
    ]);
    test_app
        .press("O")
        .type_text("Archive")
        .press("enter v j j d");

    let system_under_test = confirm_of(&test_app).message().to_owned();

    assert_eq!(system_under_test, "Move 2 lists and 1 folder to the bin?");
    test_app.press("y");
    assert!(!exists(&test_app, "Archive"));
    assert!(!exists(&test_app, "Inbox.md"));
    assert!(!exists(&test_app, "Work.md"));
}

#[test]
fn non_empty_folder_in_a_delete_selection_moves_to_the_bin_with_the_rest() {
    let mut test_app = TestApp::new(LISTS);
    test_app.press("g g v G d y");

    let system_under_test = test_app.app().workspace().entries();

    assert!(system_under_test.is_empty());
    assert!(exists(&test_app, ".trash/Home/Chores.md"));
    assert!(exists(&test_app, ".trash/Inbox.md"));
    assert!(exists(&test_app, ".trash/Work.md"));
}

#[test]
fn failed_deletions_are_reported_in_one_message() {
    let mut test_app = TestApp::new(&[("Inbox.md", ""), ("Work.md", "")]);
    test_app.press("v j d");
    test_app.remove("Work.md");
    test_app.press("y");

    let system_under_test = message_of(&test_app).unwrap();

    assert!(
        system_under_test.starts_with("deleted 1 of 2 lists; 1 failed: cannot change "),
        "{system_under_test}"
    );
    assert!(system_under_test.contains("Work.md"), "{system_under_test}");
    assert!(exists(&test_app, ".trash/Inbox.md"));
}

#[test]
fn lists_move_into_a_folder_and_back() {
    let mut test_app = TestApp::new(LISTS);
    test_app
        .press("j v j m")
        .type_text("Home")
        .press("enter")
        .press("v k m enter");
    let expected = [true, true, false, false];

    let system_under_test = [
        exists(&test_app, "Inbox.md"),
        exists(&test_app, "Work.md"),
        exists(&test_app, "Home/Inbox.md"),
        exists(&test_app, "Home/Work.md"),
    ];

    assert_eq!(system_under_test, expected);
}

#[test]
fn moved_lists_keep_their_state_and_stay_selected() {
    let mut test_app = TestApp::new(LISTS);
    test_app.press("j v j m").type_text("Home").press("enter");
    let expected = test_app.list_path("Home/Work.md");

    let system_under_test = test_app.app().lists().active().cloned();

    assert!(matches!(
        test_app.app().lists().outline().selected(),
        Some(rudo::app::lists::EntryKey::List(list)) if *list == expected
    ));
    assert_eq!(
        system_under_test,
        Some(test_app.list_path("Home/Chores.md"))
    );
}

#[test]
fn single_entry_moves_outside_visual_mode() {
    let mut test_app = TestApp::new(LISTS);
    test_app.press("G m").type_text("Home").press("enter");

    let system_under_test = test_app.read("Home/Work.md");

    assert_eq!(system_under_test, "- [ ] Write report\n- [ ] Plan week\n");
    assert!(!exists(&test_app, "Work.md"));
}

#[test]
fn move_onto_an_existing_name_is_refused() {
    let mut test_app = TestApp::new(&[
        ("Home/Inbox.md", "- [ ] Old\n"),
        ("Inbox.md", "- [ ] New\n"),
        ("Work.md", ""),
    ]);
    test_app.press("j v j m").type_text("Home").press("enter");
    let expected = "moved 1 of 2 lists; 1 failed: Home/Inbox.md already exists";

    let system_under_test = message_of(&test_app).unwrap();

    assert_eq!(system_under_test, expected);
    assert_eq!(test_app.read("Inbox.md"), "- [ ] New\n");
    assert_eq!(test_app.read("Home/Inbox.md"), "- [ ] Old\n");
    assert!(exists(&test_app, "Home/Work.md"));
}

#[rstest]
#[case::into_itself("Home", "cannot move folder Home into itself")]
#[case::missing_folder("Garden", "there is no folder Garden")]
fn invalid_move_target_keeps_the_picker_open(#[case] target: &str, #[case] expected: &str) {
    let mut test_app = TestApp::new(LISTS);
    test_app.press("g g m").type_text(target).press("enter");

    let system_under_test = folder_picker_of(&test_app).error();

    assert_eq!(system_under_test, Some(expected));
    assert!(exists(&test_app, "Home/Chores.md"));
}

#[test]
fn yank_in_the_lists_pane_explains_itself() {
    let mut test_app = TestApp::new(LISTS);
    test_app.press("v y");

    let system_under_test = message_of(&test_app);

    assert_eq!(system_under_test, Some("only tasks can be yanked"));
}

#[test]
fn move_of_a_task_selection_explains_itself() {
    let mut test_app = tasks_app();
    test_app.press("2 v m");

    let system_under_test = message_of(&test_app);

    assert_eq!(
        system_under_test,
        Some("only lists and folders can be moved into a folder")
    );
}
