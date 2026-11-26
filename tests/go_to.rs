//! Go to anything: jumping to any folder, list or task of the workspace with `ctrl-p`.

#[expect(
    dead_code,
    reason = "each test crate uses only part of the shared harness"
)]
mod common;

use std::fmt::Write;
use std::path::Path;
use std::time::{Duration, Instant};

use insta::assert_snapshot;
use ratatui::backend::TestBackend;
use rstest::rstest;
use rudo::app::go_to::GoToCandidate;
use rudo::app::lists::EntryKey;
use rudo::app::picker::{PickerCandidate, PickerPrompt};
use rudo::app::task_visibility::CompletedTasks;
use rudo::app::tasks::visible_task_items;
use rudo::app::{Focus, Mode, StatusMessage};
use rudo::config::Config;
use rudo::config::keymap::action::Context;
use rudo::storage::watch::{FsChange, PathState};
use rudo::storage::workspace::{Entry, ListPath};

use crate::common::TestApp;

/// Rows of the lists pane in display order: `Home/` (`Garden/` with `Seeds`, `Groceries`),
/// `Work/` (`Plan`), `Inbox`; the hidden `.archive/Old`, an entry in the bin and a list that
/// is not UTF-8 complete it. `Seeds`, the first list, is opened on start-up.
const WORKSPACE: &[(&str, &[u8])] = &[
    (
        "Home/Garden/Seeds.md",
        b"- [ ] Sow carrots\n\t- [ ] Water the seedlings\n",
    ),
    (
        "Home/Groceries.md",
        b"- [x] Buy bread\n- [ ] Buy milk\n- [ ] Clean the home office\n",
    ),
    ("Work/Plan.md", b"- [ ] Write report\n"),
    ("Inbox.md", b"- [ ] Call mum\n"),
    (".archive/Old.md", b"- [ ] Forgotten\n"),
    (".trash/Gone.md", b"- [ ] Thrown away\n"),
    ("Broken.md", &[0xff, 0xfe]),
];

/// A row of the finder: the label, the text and the location.
type Row<'a> = (Option<&'a str>, &'a str, Option<&'a str>);

fn start() -> TestApp {
    TestApp::from_bytes(WORKSPACE)
}

fn prompt_of(test_app: &TestApp) -> &PickerPrompt<GoToCandidate> {
    match test_app.app().mode() {
        Mode::GoTo(prompt) => prompt,
        mode => panic!("go to anything is not open, the mode is {mode:?}"),
    }
}

fn rows_of(test_app: &TestApp) -> Vec<Row<'_>> {
    prompt_of(test_app)
        .picker()
        .results(0)
        .map(|result| {
            let candidate = result.candidate();
            (candidate.label(), candidate.text(), candidate.location())
        })
        .collect()
}

fn texts_of(test_app: &TestApp) -> Vec<&str> {
    rows_of(test_app)
        .into_iter()
        .map(|(_, text, _)| text)
        .collect()
}

fn selected_entry(test_app: &TestApp) -> Option<&EntryKey> {
    test_app.app().lists().outline().selected()
}

/// Returns the names of the folders and lists the lists pane shows, in display order.
fn shown_entries(test_app: &TestApp) -> Vec<&str> {
    let app = test_app.app();
    let items = app.lists().items(app.workspace());
    app.lists()
        .outline()
        .visible_rows(&items)
        .iter()
        .filter_map(|row| row.item().entry())
        .map(Entry::name)
        .collect()
}

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

/// Returns the description of the selected task of the active list.
fn selected_task(test_app: &TestApp) -> Option<String> {
    let app = test_app.app();
    let task = *app.task_outline()?.selected()?;
    let loaded = app.active_list()?.content().ok()?;
    let node = loaded.document().tasks().find(|node| node.id() == task)?;
    Some(node.task().description().to_owned())
}

fn active_list(test_app: &TestApp) -> Option<&Path> {
    test_app.app().lists().active().map(ListPath::as_path)
}

fn message_of(test_app: &TestApp) -> Option<&str> {
    test_app.app().status_message().map(StatusMessage::text)
}

/// Returns the text of the bottom row.
fn bottom_row(backend: &TestBackend) -> String {
    let buffer = backend.buffer();
    let row = buffer.area.height - 1;
    (0..buffer.area.width)
        .map(|column| buffer[(column, row)].symbol())
        .collect()
}

#[test]
fn ctrl_p_offers_folders_and_lists_then_open_and_then_done_tasks() {
    let mut test_app = start();
    let expected: Vec<Row<'_>> = vec![
        (Some("folder"), "Home", None),
        (Some("folder"), "Home/Garden", None),
        (Some("list"), "Home/Garden/Seeds", None),
        (Some("list"), "Home/Groceries", None),
        (Some("folder"), "Work", None),
        (Some("list"), "Work/Plan", None),
        (Some("list"), "Broken", None),
        (Some("list"), "Inbox", None),
        (Some("task"), "Sow carrots", Some("Home/Garden/Seeds")),
        (
            Some("task"),
            "Water the seedlings",
            Some("Home/Garden/Seeds"),
        ),
        (Some("task"), "Buy milk", Some("Home/Groceries")),
        (
            Some("task"),
            "Clean the home office",
            Some("Home/Groceries"),
        ),
        (Some("task"), "Write report", Some("Work/Plan")),
        (Some("task"), "Call mum", Some("Inbox")),
        (Some("task"), "Buy bread", Some("Home/Groceries")),
    ];

    let system_under_test = test_app.press("ctrl-p");

    assert_eq!(rows_of(system_under_test), expected);
    assert_eq!(system_under_test.app().context(), Context::Picker);
}

#[test]
fn ctrl_p_opens_the_finder_from_the_tasks_pane_too() {
    let mut test_app = start();
    test_app.press("2");

    let system_under_test = test_app.press("ctrl-p");

    assert!(matches!(system_under_test.app().mode(), Mode::GoTo(_)));
}

#[test]
fn hidden_entries_are_searched_only_while_they_are_shown() {
    let mut test_app = start();
    let hidden = texts_of(test_app.press("ctrl-p").type_text("forgot")).len();
    test_app.press("esc H");

    let system_under_test = test_app.press("ctrl-p").type_text("forgot");

    assert_eq!(hidden, 0);
    assert_eq!(texts_of(system_under_test), ["Forgotten"]);
}

#[test]
fn the_bin_is_never_searched() {
    let mut test_app = start();
    test_app.press("H");

    let system_under_test = test_app.press("ctrl-p").type_text("thrown");

    assert_eq!(texts_of(system_under_test), Vec::<&str>::new());
}

#[test]
fn names_rank_before_paths() {
    let mut test_app = start();
    test_app.press("ctrl-p");

    let system_under_test = test_app.type_text("home");

    assert_eq!(
        texts_of(system_under_test),
        [
            "Home",
            "Clean the home office",
            "Home/Garden",
            "Home/Garden/Seeds",
            "Home/Groceries"
        ]
    );
}

#[test]
fn ties_prefer_lists_over_tasks_and_open_over_done_tasks() {
    let mut test_app = TestApp::new(&[
        ("Buy milk.md", "- [x] Buy milk\n"),
        ("Shop.md", "- [ ] Buy milk\n"),
    ]);
    test_app.press("ctrl-p");
    let expected: Vec<Row<'_>> = vec![
        (Some("list"), "Buy milk", None),
        (Some("task"), "Buy milk", Some("Shop")),
        (Some("task"), "Buy milk", Some("Buy milk")),
    ];

    let system_under_test = test_app.type_text("milk");

    assert_eq!(rows_of(system_under_test), expected);
}

#[test]
fn enter_on_a_folder_selects_it_in_the_lists_pane_and_unfolds_its_parents() {
    let mut test_app = start();
    test_app.press("2 1 z M ctrl-p").type_text("garden");
    let expected_key = EntryKey::Folder("Home/Garden".into());

    let system_under_test = test_app.press("enter");

    assert!(matches!(system_under_test.app().mode(), Mode::Normal));
    assert_eq!(system_under_test.app().focus(), Focus::Lists);
    assert_eq!(selected_entry(system_under_test), Some(&expected_key));
    assert!(shown_entries(system_under_test).contains(&"Garden"));
    assert_eq!(
        active_list(system_under_test),
        Some(Path::new("Home/Garden/Seeds.md"))
    );
}

#[test]
fn enter_on_a_list_opens_it_and_focuses_the_tasks_pane() {
    let mut test_app = start();
    test_app.press("z M ctrl-p").type_text("plan");
    let expected_key = EntryKey::List(test_app.list_path("Work/Plan.md"));

    let system_under_test = test_app.press("enter");

    assert_eq!(system_under_test.app().focus(), Focus::Tasks);
    assert_eq!(
        active_list(system_under_test),
        Some(Path::new("Work/Plan.md"))
    );
    assert_eq!(selected_entry(system_under_test), Some(&expected_key));
    assert!(shown_entries(system_under_test).contains(&"Plan"));
}

#[test]
fn enter_on_a_nested_task_opens_its_list_and_unfolds_its_parent() {
    let mut test_app = start();
    test_app.press("g g j j enter z M 1 ctrl-p");
    test_app.type_text("seedlings");

    let system_under_test = test_app.press("enter");

    assert_eq!(system_under_test.app().focus(), Focus::Tasks);
    assert_eq!(
        active_list(system_under_test),
        Some(Path::new("Home/Garden/Seeds.md"))
    );
    assert_eq!(
        selected_task(system_under_test).as_deref(),
        Some("Water the seedlings")
    );
    assert_eq!(
        shown_tasks(system_under_test),
        ["Sow carrots", "Water the seedlings"]
    );
}

#[test]
fn enter_on_a_task_hidden_with_its_completed_tasks_shows_them() {
    let mut test_app = start();
    test_app
        .press("ctrl-p")
        .type_text("milk")
        .press("enter H ctrl-p");
    test_app.type_text("bread");

    let system_under_test = test_app.press("enter");

    assert_eq!(
        selected_task(system_under_test).as_deref(),
        Some("Buy bread")
    );
    assert_eq!(
        system_under_test.app().completed_tasks(),
        CompletedTasks::Shown
    );
    assert_eq!(
        message_of(system_under_test),
        Some("completed tasks are shown")
    );
}

#[test]
fn enter_on_a_task_hidden_by_the_search_filter_reveals_it() {
    let mut test_app = start();
    test_app.press("ctrl-p").type_text("milk").press("enter /");
    test_app
        .type_text("milk")
        .press("enter ctrl-p")
        .type_text("bread");

    let system_under_test = test_app.press("enter");

    assert_eq!(
        selected_task(system_under_test).as_deref(),
        Some("Buy bread")
    );
    assert_eq!(shown_tasks(system_under_test), ["Buy bread", "Buy milk"]);
    assert_eq!(
        system_under_test
            .app()
            .task_filter()
            .map(ToString::to_string),
        Some("/milk".to_owned())
    );
}

#[test]
fn esc_closes_without_moving() {
    let mut test_app = start();
    test_app.press("ctrl-p").type_text("plan");

    let system_under_test = test_app.press("esc");

    assert!(matches!(system_under_test.app().mode(), Mode::Normal));
    assert_eq!(system_under_test.app().focus(), Focus::Lists);
    assert_eq!(
        active_list(system_under_test),
        Some(Path::new("Home/Garden/Seeds.md"))
    );
}

#[rstest]
#[case::ctrl_j_selects_the_next_match("", "ctrl-j", 1)]
#[case::ctrl_k_selects_the_previous_match("down down", "ctrl-k", 1)]
#[case::ctrl_k_keeps_the_query("down home", "ctrl-k", 0)]
fn ctrl_j_and_ctrl_k_move_the_selection(
    #[case] before: &str,
    #[case] key: &str,
    #[case] expected: usize,
) {
    let mut test_app = start();
    test_app.press("ctrl-p").type_text("o").press(before);

    let system_under_test = test_app.press(key);

    assert_eq!(
        prompt_of(system_under_test).picker().selected_index(),
        Some(expected)
    );
    assert_eq!(prompt_of(system_under_test).picker().input().value(), "o");
}

#[test]
fn ctrl_p_inside_the_finder_selects_the_previous_match() {
    let mut test_app = start();
    test_app.press("ctrl-p down down");

    let system_under_test = test_app.press("ctrl-p");

    assert_eq!(
        prompt_of(system_under_test).picker().selected_index(),
        Some(1)
    );
}

#[test]
fn printable_keys_go_to_the_input() {
    let mut test_app = start();
    test_app.press("ctrl-p");

    let system_under_test = test_app.press("j k q");

    assert_eq!(prompt_of(system_under_test).picker().input().value(), "jkq");
    assert!(!system_under_test.app().should_quit());
}

#[test]
fn go_to_can_be_rebound_to_ctrl_space() {
    let config =
        Config::from_yaml("keybindings:\n  go-to: ctrl-space\n", Path::new("c.yaml")).unwrap();
    let mut test_app = TestApp::with_config(&[("Inbox.md", "- [ ] Call mum\n")], config);
    let old_key_opens = matches!(test_app.press("ctrl-p").app().mode(), Mode::GoTo(_));

    let system_under_test = test_app.press("ctrl-space");

    assert!(!old_key_opens);
    assert!(matches!(system_under_test.app().mode(), Mode::GoTo(_)));
}

#[test]
fn task_reloaded_while_the_finder_is_open_is_found_by_its_text() {
    let mut test_app = start();
    test_app.press("ctrl-p").type_text("write");
    test_app.write("Work/Plan.md", "- [ ] Plan the week\n- [ ] Write report\n");
    let changes = [FsChange::new("Work/Plan.md", PathState::File)];
    test_app.run(|app| app.on_fs_changed(&changes));

    let system_under_test = test_app.press("enter");

    assert_eq!(
        selected_task(system_under_test).as_deref(),
        Some("Write report")
    );
}

#[test]
fn task_removed_while_the_finder_is_open_is_reported() {
    let mut test_app = start();
    test_app.press("ctrl-p").type_text("write");
    test_app.write("Work/Plan.md", "- [ ] Plan the week\n");
    let changes = [FsChange::new("Work/Plan.md", PathState::File)];
    test_app.run(|app| app.on_fs_changed(&changes));

    let system_under_test = test_app.press("enter");

    assert_eq!(
        prompt_of(system_under_test).error(),
        Some("task Write report is gone")
    );
}

#[test]
fn finder_popup_shows_kinds_matches_and_lists() {
    let mut test_app = start();
    test_app.press("ctrl-p").type_text("home");

    let system_under_test = test_app.render(80, 24);

    assert_snapshot!(system_under_test);
}

#[test]
fn bottom_bar_shows_the_keys_of_the_picker() {
    let mut test_app = start();
    test_app.press("ctrl-p");

    let system_under_test = bottom_row(&test_app.render(80, 24));

    assert!(
        system_under_test
            .starts_with(" enter accept  ctrl-j next  ctrl-k previous  tab complete  esc cancel "),
        "{system_under_test}"
    );
}

/// The number of lists and of tasks in each of them of the generated workspace.
const LISTS: usize = 100;
const TASKS_PER_LIST: usize = 100;
/// Generous bounds that an unoptimized test build on a slow machine stays far below (about
/// 15 ms to open and 80 ms for the slowest keystroke on a laptop, 1 ms and 4 ms optimized);
/// they catch a finder that matches more than once per keystroke or copies the workspace per
/// row.
const OPEN_BUDGET: Duration = Duration::from_secs(1);
const KEYSTROKE_BUDGET: Duration = Duration::from_millis(500);

/// Starts the app on a workspace of [`LISTS`] lists in ten folders with [`TASKS_PER_LIST`]
/// tasks each.
fn generated_workspace() -> TestApp {
    let files: Vec<(String, String)> = (0..LISTS)
        .map(|list| {
            let mut content = String::new();
            for task in 0..TASKS_PER_LIST {
                writeln!(content, "- [ ] Task {task} of list {list} with some words")
                    .expect("writing to a String never fails");
            }
            (format!("Folder {}/List {list}.md", list % 10), content)
        })
        .collect();
    let files: Vec<(&str, &str)> = files
        .iter()
        .map(|(path, content)| (path.as_str(), content.as_str()))
        .collect();
    TestApp::new(&files)
}

#[test]
fn finder_over_ten_thousand_tasks_opens_within_budget() {
    let mut test_app = generated_workspace();
    let expected_candidates = LISTS * TASKS_PER_LIST + LISTS + 10;

    let system_under_test = {
        let opened = Instant::now();
        test_app.press("ctrl-p");
        opened.elapsed()
    };

    assert!(system_under_test < OPEN_BUDGET, "{system_under_test:?}");
    let candidates = prompt_of(&test_app).picker().candidate_count();
    assert_eq!(candidates, expected_candidates);
}

#[test]
fn finder_over_ten_thousand_tasks_ranks_every_keystroke_within_budget() {
    let mut test_app = generated_workspace();
    test_app.press("ctrl-p");
    let query = "task 42 of list 7 ";
    let expected_best = "Task 42 of list 7 with some words";

    let system_under_test: Vec<Duration> = query
        .chars()
        .map(|character| {
            let typed = Instant::now();
            test_app.type_text(character.encode_utf8(&mut [0; 4]));
            typed.elapsed()
        })
        .collect();

    let slowest = system_under_test.iter().max().unwrap();
    assert!(*slowest < KEYSTROKE_BUDGET, "{slowest:?}");
    assert_eq!(texts_of(&test_app).first(), Some(&expected_best));
}
