# Architecture

This document describes how RuDo is built and why. The user-facing behaviour (file format,
default keymap, configuration) is specified in the [user documentation](README.md); the
coding rules are in [AGENTS.md](../AGENTS.md). When this document and the code disagree, fix
one of them in the same change.

## Principles

- **Functional core, imperative shell.** Everything that decides *what* happens is pure:
  parsing, document edits, key resolution, the application state machine and rendering into
  a `ratatui` buffer. Only a thin shell touches the terminal, the clock and the file system.
  The core is therefore tested with fast, output-based unit tests and needs no mocks.
- **The Elm architecture.** Input becomes an `Action`; `App::update(action)` changes the
  in-memory `App` and returns a list of `Effect`s (save a file, create a folder, open the
  editor). The shell executes the effects, which report their results back to the `App`.
  `view` renders the state and never changes it.
- **Parse, don't validate.** Text from outside (configuration, Markdown files, typed names,
  date expressions, key notation) is parsed once at the boundary into types that can only
  hold valid values: `KeySequence`, `EntryName`, `Date`, `Recurrence`, `Priority`,
  `IndentStyle`.
- **One source of truth for every fact** (DRY):

  | Fact                       | Source of truth                   | Derived from it                                              |
  | -------------------------- | --------------------------------- | ------------------------------------------------------------ |
  | what a user can do         | `Action` enum                     | config validation, help screen, key hints, documentation guard tests |
  | which keys trigger what    | effective `Keymap`                | key resolution, help screen, bottom bar hints, empty states  |
  | default configuration      | `assets/default-config.yaml`      | `Config::default()` (guard test), `rudo config default`      |
  | colours                    | `Theme` with the `Slot` enum      | every widget, documentation guard test; no widget hard-codes a colour |
  | a task's Markdown syntax   | `markdown::task_line`             | parsing and rendering of every task line                     |
  | tree navigation            | `ui::outline::Outline`            | lists pane, tasks pane, help screen scrolling                |
  | screen geometry            | `ui::layout`                      | the drawn areas and the viewport heights used for scrolling  |

- **Lossless storage.** A file that RuDo loads and saves without an edit is byte-identical.
  An edit re-renders only the task lines it touched.
- **No surprises on disk.** Every change is saved immediately (autosave) with an atomic write.
  Changes from other programs always win over RuDo's in-memory state.

## Module layout

Single package, library plus binary. The binary only wires things together; all logic lives
in the library so it can be unit tested and documented.

```text
src/
  main.rs                  binary: parse CLI, set up logging, load config, run (anyhow)
  lib.rs                   crate docs and module tree
  cli.rs                   clap definitions: options, `config` and `bin` subcommands, the
                           lines of `rudo bin list`
  logging.rs               file logging with `tracing-subscriber`, `RUDO_LOG` filter
  paths.rs                 config, data and state directories per Platform (XDG, Windows
                           folders), `~` expansion, workspace paths shown and stored with `/`
  platform.rs              Platform: the operating systems RuDo tells apart
  user_docs.rs             test only: sections, tables and code spans of docs/ for guard tests
  config/
    mod.rs                 Config, config file lookup, parsing with located error reports
    root.rs                WorkspaceRoot: `--root`, `workspace.root` or the default folder
    settings.rs            Settings: config, theme and keymap loaded and validated together
    theme.rs               Theme, Slot, StyleConfig, ColourMode (`NO_COLOR`)
    keymap/
      mod.rs               module tree
      key.rs               KeyChord, KeySequence: notation parsing and Display
      action.rs            Action, Context, descriptions, labels, default keys
      keymap.rs            effective Keymap: defaults + overrides, conflict validation
      resolver.rs          KeyResolver: multi-key sequence state machine
  domain/
    mod.rs                 module tree; nothing here reads the clock
    date.rs                due date expressions, DueClass and relative formatting
    recurrence.rs          recurrence rules and next occurrence
    priority.rs            Priority levels and stepping through them
    tags.rs                Obsidian tag recognition
  markdown/
    mod.rs                 module tree
    task_line.rs           one task line: parse and render, TaskStatus, InlineField
    document/
      mod.rs               TodoDocument: lossless file model and task tree, EditOutcome
      insert.rs            inserting, removing, copying and pasting task blocks
      reorder.rs           moving, indenting and outdenting task blocks
      sort.rs              sorting a task and its siblings by a SortKey
      content.rs           description, status and inline field edits
  storage/
    mod.rs                 module tree
    bin.rs                 BinPath, TrashedEntry, BinEntry; moving lists and folders into the
                           bin and back, its index, scanning, deleting from and emptying it
    fingerprint.rs         content fingerprints for detecting external changes
    workspace.rs           Workspace: scan, rescan and reload of the tree of folders and lists
    save.rs                atomic saving that refuses to overwrite external changes
    entries.rs             EntryName; create, rename, move and delete lists and folders
    watch.rs               WorkspaceWatcher: debounced recursive watching, FsChange
  app/
    mod.rs                 module tree and re-exports (App, Focus, Mode, Effect, execute)
    state.rs               App, Focus, Mode, the context and viewport heights
    update.rs              App::handle_key, handle_paste, update(Action) -> Vec<Effect>, edit_list
    effect.rs              Effect and execute, its executor, and the result handlers
    status.rs              StatusMessage, Severity and when messages expire
    editor.rs              open-in-editor: choosing the editor, its command line with the line
    reload.rs              external changes: App::on_fs_changed, reload_list, rescan, conflicts
    history.rs             undo and redo stacks of document snapshots and EntryOperations
    register.rs            the yank register holding detached TaskBlocks
    filter.rs              smart-case matching shared by every filter
    fuzzy.rs               fuzzy matching with nucleo-matcher: FuzzyQuery, FuzzyMatcher, scores
                           and matched characters, names ranked before paths
    picker.rs              generic fuzzy picker state: Picker, PickerCandidate, ranked results
    folder_picker.rs       folder picker (Mode::FolderPicker): FolderPurpose, moving and
                           restoring into the chosen folder
    go_to.rs               go to anything (Mode::GoTo): folder, list and task candidates,
                           jumping to the chosen one
    search.rs              pane search (Mode::Search): PaneFilter per pane and per list
    help.rs                help screen state (Mode::Help): scrolling and filter input
    prompt.rs              input prompt state (Mode::Prompt): PromptKind, validation on submit
    confirm.rs             confirmation dialog state (Mode::Confirm): ConfirmKind
    visual.rs              visual mode (Mode::Visual): resolving and acting on the selection
    lists.rs               lists pane state: workspace tree navigation, active list
    bin.rs                 bin view (BinView): browsing, restoring, deleting from and emptying
                           the bin
    tasks.rs               tasks pane state: task tree navigation per list
    task_input.rs          inline task editor (Mode::InlineEdit): adding and editing tasks
    task_fields.rs         reading and writing inline fields of the selected task
    task_toggle.rs         marking the selected task done or open again
    task_priority.rs       raising and lowering the priority of the selected task
    task_due.rs            the due date prompt with its live preview
    task_recurrence.rs     the recurrence prompt, the next occurrence on completion
    task_reorder.rs        moving, indenting and outdenting the selected task
    task_delete.rs         deleting the selected task, or several, with their subtasks
    task_sort.rs           sorting the selected task and its siblings
    task_yank.rs           yanking the selected task and pasting the register
    task_visibility.rs     showing and hiding completed tasks (CompletedTasks)
    entry_visibility.rs    showing and hiding hidden folders and lists (HiddenEntries)
  ui/
    mod.rs                 view(&App, &mut Frame)
    layout.rs              responsive layout: Screen, pane, bottom bar and overlay areas
    outline.rs             generic tree navigation: selection, visual selection, folding,
                           scrolling, prune
    lists_pane.rs          workspace tree with `done/total` counters
    tasks_pane.rs          task tree with metadata
    pane_message.rs        messages of empty panes that name the bound keys
    status_bar.rs          key hints, pending keys, messages, the search prompt
    text_input.rs          TextInput, EditCommand, EDITING_KEYS and the TextInputView widget
    popup.rs               input popup and confirmation dialog
    picker.rs              PickerPopup: input, ranked results with highlights, `top-bottom/total`
    help.rs                help content model generated from the effective keymap
    help_overlay.rs        help overlay: rows, scrollbar, filter line
    highlight.rs           search match highlighting inside styled row text
  runtime/
    mod.rs                 module tree
    terminal.rs            TerminalGuard: terminal setup and restore, suspend for the editor
    event.rs               Event, InputThread and batching of the event channel
    run.rs                 the event loop and the editor hand-over
```

## Data flow

```text
 terminal input (InputThread) ─┐
                               ├─► mpsc channel ─► runtime::run ─► next_batch ─┐
 file watcher (FsChanged)     ─┘                                               │
                                                                               ▼
           App::handle_key ─► KeyResolver ─► App::update(Action) ─► Vec<Effect>
           App::handle_paste, App::on_fs_changed, App::resize ─┘         │
                                                                         ▼
                      runtime: OpenInEditor ◄── perform ──► app::execute (storage::*)
                                                                         │
                                        results reported to App ◄────────┘
 before waiting for the next batch: app.tick(now), app.set_today(date), terminal.draw(view)
```

- Events are read on a dedicated thread and sent over a `std::sync::mpsc` channel as
  `Event::Key`, `Resize`, `Paste` or `FsChanged`. RuDo has no async runtime; the work is a
  single interactive loop plus blocking file I/O.
- The loop draws, then blocks on the channel and drains all queued events (`next_batch`)
  before it draws again. There is no tick event and no busy loop: the wait times out only at
  `App::next_deadline` (while a timed message is visible) and at the start of the next day,
  so that today's date is updated.
- `App::handle_key` resolves the chord in `App::context()`, which is derived from the mode
  and the focus, and calls `App::update` for a complete sequence. A key that no action is
  bound to is typed into the open text input, if any. Pasted text (bracketed paste) only
  goes into an open text input and is never interpreted as keys.
- The `Mode` decides what takes the keys: `Normal` (the focused pane), `Help(HelpOverlay)`,
  `Prompt(PromptOverlay)` (input popup), `FolderPicker(FolderPicker)` (fuzzy picker of a
  folder), `GoTo(PickerPrompt<GoToCandidate>)` (go to anything), `Confirm(ConfirmOverlay)`,
  `InlineEdit(InlineEdit)` (a row of the tasks pane), `Search(Focus)` (the search prompt in
  the bottom bar) and `Visual` (a selection of several rows of the focused pane or of the bin
  view). `Prompt`, `InlineEdit`, `Search` and the open help filter resolve keys in
  `Context::Input`, `FolderPicker` and `GoTo` in `Context::Picker` (`submit` and `cancel` as
  in `Input`, plus `picker-next`, `picker-previous` and `complete`), `Visual` in
  `Context::Visual`, or in `Context::BinVisual` while the bin view is open, so that
  `restore-from-bin` resolves on a selection in the bin but not on one in the panes.
- A picker (`app::picker::Picker<C>`) is generic over its candidates (`PickerCandidate`: the
  text shown, matched and completed to, where its name starts, and optionally its theme slot,
  a label before it and a location after it). `PickerPrompt<C>` adds the error of a refused
  choice, which every change hides; `PickerCommand` moves the selection or completes in
  whichever picker is open. It ranks all candidates once per change of the typed text with
  `app::fuzzy` (nucleo-matcher, smart case, matches in the name before matches in the rest of
  the path) and keeps the results; moving the selection, completing with `tab` (which cycles
  while the text equals the selection) and drawing do not match again.
  `ui::picker::PickerPopup` draws only the visible results and scrolls without stored state,
  like `TextInputView`. The folder picker offers the root (`/`) and the folders the lists
  pane can show, collected when it opens, and checks the choice with `check_move_target` on
  `enter`. Go to anything collects the folders and lists the lists pane can show, then the
  open and then the completed tasks of the readable lists, so that equal scores keep that
  order; on `enter` it looks the target up again (a reloaded task by its text, since a reload
  gives new task ids) and reports one that is gone.
- The visual selection is an anchor key in the pane's `Outline` (or the `BinView`'s) plus its
  cursor; the selected keys are derived from the visible rows whenever they are needed, so
  folds, filters, hidden completed tasks and hidden entries are respected and a reload that
  removes the anchor ends
  visual mode (`App::end_visual_mode_without_anchor`). Every action on the selection ends visual mode
  before it acts or opens a dialog.
- `Effect` is one of `Save`, `CreateEntry`, `RenameEntry`, `DeleteEntries`, `MoveEntries`,
  `OpenInEditor`, `ReloadList`, `Rescan`, `StepEntries`, `RestoreEntries`, `DeleteFromBin`
  and `EmptyBin`. The bin view restores one entry or a selection as one `RestoreEntries` (one
  undo step) and deletes it permanently as one `DeleteFromBin`; both report partial failures
  in one message. Every deletion of lists and
  folders, a single one too, is one `DeleteEntries` (asked for by
  `App::ask_to_delete_entries`, performed by `delete_entries` in `app::effect`, which moves
  the entries into the bin with `storage::bin::move_to_bin`), and every move one `MoveEntries` (`move_entries`, which
  renames with `storage::entries::move_into` and follows the entry through
  `App::apply_entry_change`); both report all failures in one message. `app::execute` performs all but `OpenInEditor` and reports the
  result to the `App`; failures become a `StatusMessage`, never an abort.
- The input thread polls with a short timeout and stops when its `InputThread` handle is
  dropped, so the terminal can be handed to an external editor and input resumed afterwards
  by spawning a new one.
- `Effect::OpenInEditor` needs the terminal, so the runtime performs it instead of `execute`:
  it stops the input thread (`InputThread::stop` returns the sender), restores the terminal
  with `TerminalGuard::suspend`, runs the `EditorCommand` and waits, sets the terminal up
  again, spawns a new input thread on the same channel and calls `App::reload_list`, which
  replaces the document only if the file's fingerprint changed. Only the runtime reads
  `$VISUAL` and `$EDITOR`; choosing the editor and building its command line are pure.
- `storage::watch::WorkspaceWatcher` watches the workspace recursively with
  `notify-debouncer-full` (200 ms) and sends every batch as `Event::FsChanged(Vec<FsChange>)`
  into the same channel. Each `FsChange` is a path relative to the root plus what is there
  now (`PathState::File`, `Directory` or `Missing`), inspected on the watcher thread, so that
  `App::on_fs_changed` can classify purely against the in-memory workspace: a file at a known
  list becomes `Effect::ReloadList` (also for editors that save by renaming a temporary file),
  any other change of the tree one `Effect::Rescan`; the bin (`workspace.bin`, `.trash/` by
  default, checked with `BinPath::contains` like in the scan), non-`.md` files and
  new directories in folders the tree left out (such as `.git/objects/ab`) are ignored. Hidden
  paths count like any other: the workspace always holds hidden entries (`Entry::is_hidden`,
  any path component starting with `.`), the scan leaves out only hidden folders without lists
  and the bin, and the lists pane hides them with `outline::prune` while `HiddenEntries` is
  `Hidden`, so toggling them needs no rescan. RuDo's own writes are reported as well and change
  nothing: the workspace already matches the disk and `App::reload_list` compares fingerprints. `App::rescan` scans the tree
  again with `Workspace::rescan`, which keeps the document of every list whose fingerprint did
  not change, so selections, folds and undo history stay keyed by path; changed lists are
  reloaded and the state of removed lists is forgotten. If the watcher cannot start
  (for example the inotify limit), RuDo runs on and shows `live reload disabled: <reason>`.
- A reloaded list keeps the selection on the line of the previously selected task (or the
  last task before it), opens its folds, forgets its undo history and shows
  `<list> reloaded`. A list file that cannot be loaded any more shows its load error.
- Only `KeyEventKind::Press` and `Repeat` are handled; `Release` events (Windows, kitty
  keyboard protocol) are dropped at the boundary.
- The runtime reports the terminal size with `App::resize` at start-up and on every resize
  event. `ui::layout` is the single source of the geometry: `view` draws into the areas it
  returns, and `App` derives the viewport height of a pane or overlay from the same functions
  (`ui::layout::pane_content`, `ui::layout::help_overlay`), so navigation moves by exactly
  the height that is drawn. There are no viewport constants. `App::resize` refreshes every
  outline with the new heights, so the selections stay visible, and `view` renders from the
  stored offsets without changing state. Below 60 columns only the focused pane is shown
  (`Screen::FocusedOnly`), below 24 columns or 5 rows only a short message.
- Text inputs scroll horizontally without stored state: `TextInputView` computes the scroll
  offset from the text, the cursor and the width of its row on every render (just far
  enough to show the cursor cell). A `TextInput` therefore holds only its text and cursor,
  `App` never needs to know the width of an input, and `view` renders inputs and
  `InputPopup`s from `&TextInput` and `&Prompt`. The price is that the cursor rests at the
  right edge while moving left through text scrolled to its end, instead of the text staying
  put until the cursor reaches the left edge.
- `TerminalGuard` sets the terminal up with `ratatui::try_init`, whose panic hook restores
  it, enables bracketed paste and restores everything when dropped. Use crossterm through
  `ratatui::crossterm` so there is exactly one crossterm version.

## Document model

```rust
pub struct TodoDocument {
    byte_order_mark: bool,
    lines: Vec<DocumentLine>,  // every line of the file, in order
    line_ending: LineEnding,   // Lf or CrLf, the dominant one, for inserted lines
    next_task_id: u32,         // counter for fresh TaskIds
}

struct DocumentLine {
    kind: LineKind,
    ending: Option<LineEnding>, // per line; None only for a last line without newline
}

enum LineKind {
    Raw(String),               // anything that is not a task, kept verbatim
    Task(TaskId, TaskLine),    // the id is stable for the lifetime of the loaded document
}

pub struct TaskLine {          // a plain value: markdown::task_line knows nothing of ids
    indent: String,            // original indentation, verbatim
    marker: ListMarker,        // `-`, `*`, `+` or `1.` / `1)`
    status: TaskStatus,        // Todo, Done, InProgress, Cancelled, Other(char)
    description: String,       // text without inline fields; tags stay in the text
    fields: Vec<InlineField>,  // `[key:: value]` in original order, unknown keys included
    original: Option<String>,  // verbatim source line until the task is edited
}
```

- The task tree is implicit: a task is the child of the nearest preceding task with a smaller
  indentation width (a tab counts as four columns). A task's *block* is the task line plus all
  following lines that are indented deeper, including continuation lines and blank lines
  between them. Moving, deleting, yanking, sorting and indenting always operate on whole
  blocks.
- The indent style is detected on demand (`indent_style(fallback)`): the first indented task
  decides between tab and spaces, the space width is the smallest step between a task and its
  parent; without indented tasks the configured `tasks.indent` is used.
- `TaskId` is assigned when a document is parsed and survives edits, so the UI keeps the
  selection on the same task while lines shift. An external reload parses the file with
  `TodoDocument::reparse`, whose ids continue after those of the previous version, so an id
  held by an open inline editor or prompt never names a different task after a reload: it
  finds its task gone and says so.
- Fenced code blocks and front matter are tracked while parsing; task-like lines inside them
  stay `Raw`.
- RuDo writes Dataview inline fields: `[due:: …]`, `[priority:: …]`, `[repeat:: …]`,
  `[completion:: …]`. A re-rendered task is written as
  `{indent}{marker} [{status}] {description}` followed by ` [key:: value]` for every field.
  Setting a field replaces the first one with its key in place (and drops duplicates) or
  appends it.

## Keymap model

- `Action` is a closed enum (`strum::EnumIter`, kebab-case names). Every action knows its
  description, its short label for the bottom bar, the `Context`s it is valid in and its
  default key sequences, each through an exhaustive `match`, so adding an action fails to
  compile until all four are defined.
- `Context` is one of `Lists`, `Tasks`, `Visual`, `Bin`, `BinVisual`, `Help`, `Dialog`,
  `Input`, `Picker`. Global actions are valid in `Lists` and `Tasks` (`help`, `quit`, `undo` and `redo`
  also in `Bin`); navigation actions additionally in `Visual`, `Bin`, `BinVisual` and `Help`,
  `search` in `Help`. Keys are bound per action, so the selection actions of `Visual`
  (`delete-selection` on `d`) are separate from those of the panes (`delete` on `d d`).
- The effective `Keymap` is built once at start-up from the defaults and the user's
  overrides and validated per context: no key sequence bound twice, no binding that is a prefix
  of another, no binding in `Input` or `Picker` that starts with a printable key. Validation errors name
  both actions.
- `KeyResolver` consumes one `KeyChord` at a time and answers `Action`, `Pending`,
  `Cancelled` (`esc` while a sequence is pending) or `NoMatch`. A chord that breaks a pending
  sequence is retried once as the start of a new one, and pending chords never carry over into
  another `Context`. Because prefixes are rejected at load time it needs no timeout.

## Persistence

- Features change documents only through `App::edit_list`, the single mutation choke point:
  it applies the edit and returns `Effect::Save` when the document changed. The executor
  writes atomically: a temporary file (`.tmp…`, without the `.md` extension, so the scanner and
  the watcher ignore it) in the same directory, `fsync`, then `rename` over the target. Symlinks are resolved first so
  the link survives, and the original file permissions are kept.
- Text typed into a task row (`Mode::InlineEdit`) does not touch the document: the tasks pane
  shows a placeholder row, and submitting applies the whole change with one `edit_list` call,
  so it is saved once and undone in one step; cancelling leaves the file byte-identical.
- `edit_list` also records the document and the selected task before every changing edit in
  `app::history`: one undo stack and one redo stack for all lists, each bounded to 100
  entries, and a new change clears the redo stack. Undo and redo swap the snapshot with the
  document of its list, bypassing `edit_list`, open that list if it is not the active one,
  select the remembered task and save. A list's history follows it when it is renamed, is
  kept while the list is in the bin (it comes back at the same path) and is forgotten when
  the list is removed or reloaded from disk by another program.
- Creating, renaming, moving and deleting lists and folders are recorded in the same history
  as the `history::EntryOperation` that reverts them: `Trash` (undo of a new entry),
  `Restore` with the `TrashedEntry`s of a deletion and the list that was active, or
  `Relocate` (undo of a rename or move); a bulk operation is one entry. Undoing or redoing
  one is `Effect::StepEntries`, because it touches the disk; performing an operation yields
  its reversal for the opposite stack, and the part that failed (a restore whose path was
  taken, a file removed by another program) goes back onto its own stack with an error
  message, so nothing is ever overwritten and the next step tries it again. Restored entries
  enter the workspace through `App::rescan`.
- `move_to_bin` and `restore` keep the index of the bin, `.trash/.rudo-bin.json`
  (`serde_json`, a list of `TrashedEntry` with original path, location in the bin and
  deletion time), written atomically. Its paths are stored with `/` on every platform
  (`paths::portable`) and read with `/` or `\` (`paths::parse_portable`). The folder is the truth: `storage::bin::scan_bin` lists
  what is there, attaches what the index knows and shows unknown entries (notes Obsidian
  deleted) without an original path, and a missing or corrupt index only loses that
  information. `Workspace` holds the scanned bin (`Workspace::bin_entries`), scanned with the
  workspace and kept in step by RuDo's own changes (`Workspace::trash`,
  `Workspace::remove_bin_entry`, `Workspace::rescan_bin` after emptying); the live reload
  ignores it. While the bin is not empty, the lists pane ends with the row `EntryKey::Bin`,
  which opens the `BinView` in place of the tree (`Context::Bin`). Restoring from the view is
  `Effect::RestoreEntries`, which shares `restore_entries` with the undo of a deletion and
  records the move back into the bin as a new undo step. The tasks pane renders the selected
  entry of the bin read-only from `Workspace::bin_entries`; the keys of `Context::Bin` hold no
  task action and cannot move the focus, so no edit reaches `App::edit_list`.
- Every loaded list remembers the `Fingerprint` of the content RuDo last loaded or saved.
  Before writing, `storage::save` compares the file on disk with it. If it differs, RuDo does
  not overwrite it: `App::save_conflicted` reloads the file and reports
  `<list> changed on disk; your last change was not applied`. After saving, the fingerprint
  of the new content is remembered, which is also how reloads recognise RuDo's own writes.
- Deleting lists and folders asks first and moves them into the bin. The only permanent
  deletions are `storage::bin::delete_from_bin` and `storage::bin::empty_bin`, which delete
  with `delete_list` and `delete_folder` of `storage::entries`, asked for by the bin view
  (`Effect::DeleteFromBin`, `Effect::EmptyBin`) and `rudo bin empty`; they are not undoable
  and drop the restores of what they deleted from the history. Deleting the tasks of
  a visual selection removes their blocks from the bottom of the document up in one
  `edit_list` call, so it is one undo step.

## Testing strategy

Tests follow the classical school (*Unit Testing: Principles, Practices, and Patterns*): test
observable behaviour through public APIs, prefer output-based tests, and do not mock what can
run in-process.

| Layer                              | How it is tested                                                      |
| ---------------------------------- | --------------------------------------------------------------------- |
| parsing, domain types, edits       | unit tests with `rstest` tables; `proptest` for round trips           |
| `App::update`                      | build a state, apply actions, assert on the resulting state and effects |
| widgets and layout                 | `TestApp::render(width, height)` into a `TestBackend`; `insta` text snapshots (`just snap`), colours asserted on single cells |
| storage                            | the real file system under `tempfile::TempDir`                        |
| user scenarios                     | `tests/` drive a `TestApp` (`tests/common`) with `press` and `type_text` against a `TempDir` workspace and assert on the files and the rendered screen; the user journeys in `tests/scenarios.rs` compare the files with golden files in `tests/fixtures/` |
| documentation                      | guard tests read `docs/keybindings.md` and `docs/configuration.md` through `src/user_docs.rs`: every `Action` with its default keys, every `Slot`, the colour names and the configuration example; others check the default configuration file; `tests/doc_links.rs` checks every relative link and anchor in the Markdown files |
| terminal and event loop            | kept too thin to need unit tests (humble object); `just smoke` drives the real binary in a pseudo-terminal, and [`QA.md`](QA.md) is the manual checklist before a release |

Every test has the three phases required by AGENTS.md: setup, `system_under_test`, assertions.
Time is never read inside the core: functions that need today's date take it as a parameter,
and the shell passes `jiff::ZonedDateTime::now().date()`. Static musl builds bundle the time
zone database for systems without `/usr/share/zoneinfo`; `tests/time_zone.rs`, compiled only
for musl and run by `just test-musl`, checks it.

`just check-targets` lints the code for `x86_64-pc-windows-msvc`, `aarch64-apple-darwin` and
`x86_64-unknown-freebsd` on any system, since checking needs neither a linker nor an SDK of the
target. CI runs the lints and the tests on Windows and macOS as well, and the tests in a
FreeBSD virtual machine (see [Development](development.md#continuous-integration));
`.gitattributes` keeps the golden files byte-exact on Windows checkouts.

## Conventions

- No pictographic symbols in the interface. Box-drawing characters and simple geometric
  shapes (`▸`, `▾`, `•`) are fine; emoji and emoji-like dingbats are not.
- Logging goes to a file only (`$XDG_STATE_HOME/rudo/rudo.log`, `%LOCALAPPDATA%\rudo\rudo.log`
  on Windows); writing to stdout or stderr would corrupt the terminal interface.
- Platform differences are explicit: every function whose result differs between Linux,
  macOS and Windows takes a `platform::Platform` (and the environment values it needs) as a
  parameter, so that every variant is unit tested on every machine (`BaseDir::resolve`,
  `choose_editor`, `split_command_line`). Only thin wrappers in the shell call `Platform::current()` or read the
  environment (`BaseDir::app_dir`, the editor hand-over in `runtime::run`). Paths relative
  to the workspace root are displayed with `paths::portable`, so they show `/` everywhere;
  absolute paths of the operating system keep its own separator.
- Program output of command line subcommands (`rudo config default`) goes to stdout and is
  not logging.
- Dependencies are added with `cargo add`, with `default-features = false` where possible,
  and pass `cargo deny check`.
