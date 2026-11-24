# Architecture

This document describes how RuDo is built and why. The user-facing behaviour (file format,
default keymap, configuration) is specified in the [README](../README.md); the coding rules
are in [AGENTS.md](../AGENTS.md). When this document and the code disagree, fix one of them
in the same change.

## Principles

- **Functional core, imperative shell.** Everything that decides *what* happens is pure:
  parsing, document edits, key resolution, the application state machine and rendering into
  a `ratatui` buffer. Only a thin shell touches the terminal, the clock and the file system.
  The core is therefore tested with fast, output-based unit tests and needs no mocks.
- **The Elm architecture.** Input becomes an `Action`; `App::update(action)` changes the
  in-memory `App` and returns a list of `Effect`s (save a file, create a folder, open the
  editor). The shell executes the effects and feeds their results back as events. `view`
  renders the state and never changes it.
- **Parse, don't validate.** Text from outside (configuration, Markdown files, typed names,
  date expressions, key notation) is parsed once at the boundary into types that can only
  hold valid values: `KeySequence`, `ListName`, `DueDate`, `Recurrence`, `Priority`.
- **One source of truth for every fact** (DRY):

  | Fact                       | Source of truth                   | Derived from it                                              |
  | -------------------------- | --------------------------------- | ------------------------------------------------------------ |
  | what a user can do         | `Action` enum                     | config validation, help screen, key hints, README guard test |
  | which keys trigger what    | effective `Keymap`                | key resolution, help screen, bottom bar hints, empty states  |
  | default configuration      | `assets/default-config.yaml`      | `Config::default()` (guard test), `rudo config default`      |
  | colours                    | `Theme` with the `Slot` enum      | every widget; no widget hard-codes a colour                  |
  | a task's Markdown syntax   | `markdown::task_line`             | parsing and rendering of every task line                     |
  | tree navigation            | `ui::outline::Outline`            | lists pane, tasks pane, help screen scrolling                |

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
  cli.rs                   clap definitions: options and `config` subcommands
  logging.rs               file logging with `tracing-subscriber`, `RUDO_LOG` filter
  paths.rs                 XDG config, data and state directories, `~` expansion
  config/
    mod.rs                 Config, loading, error reporting
    root.rs                WorkspaceRoot: `--root`, `workspace.root` or the default folder
    settings.rs            Settings: config, theme and keymap loaded and validated together
    theme.rs               Theme, Slot, StyleConfig, NO_COLOR
    keymap/
      key.rs               KeyChord, KeySequence: notation parsing and Display
      action.rs            Action, Context, descriptions, default keys
      keymap.rs            effective Keymap: defaults + overrides, conflict validation
      resolver.rs          multi-key sequence state machine
  domain/
    date.rs                due date expressions and relative formatting
    recurrence.rs          recurrence rules and next occurrence
    priority.rs            Priority levels
    tags.rs                Obsidian tag recognition
  markdown/
    task_line.rs           one task line: parse and render
    document/
      mod.rs               TodoDocument: lossless file model and task tree, EditOutcome
      insert.rs            inserting, removing, copying and pasting task blocks
      reorder.rs           moving, indenting and outdenting task blocks
      content.rs           description, status and inline field edits
  storage/
    fingerprint.rs         content fingerprints for detecting external changes
    workspace.rs           scanning the workspace into a tree of folders and lists
    save.rs                atomic saving, external modification detection
    entries.rs             create, rename and delete lists and folders
    watch.rs               file system watching
  app/
    state.rs               App, Focus, Mode
    lists.rs               lists pane state: workspace tree navigation, active list
    prompt.rs              input prompt state (Mode::Prompt): PromptKind, validation on submit
    status.rs              StatusMessage, Severity and when messages expire
    update.rs              App::update(Action) -> Vec<Effect>
    effect.rs              Effect and its executor
    history.rs             undo and redo
    register.rs            yank register
    filter.rs              smart-case search and filtering
    help.rs                help screen state (Mode::Help): scrolling and filter input
  ui/
    mod.rs                 view(&App, &mut Frame)
    outline.rs             generic tree navigation: selection, folding, scrolling
    layout.rs              responsive layout: pane, bottom bar and overlay areas, focus border
    lists_pane.rs          workspace tree
    tasks_pane.rs          task tree with metadata
    status_bar.rs          key hints, pending keys, messages
    text_input.rs          single-line text editor widget
    popup.rs               input popup and confirmation dialog
    help.rs                help content model
    help_overlay.rs        help overlay: rows, scrollbar, filter line
  runtime/
    terminal.rs            terminal setup and restore, panic safety
    event.rs               merging terminal, tick and file system events
    run.rs                 the event loop
```

The layout is a target, not a mandate. Create a module when the first issue needs it, and
keep the names above unless there is a reason to deviate.

## Data flow

```text
 terminal input ─┐
 file watcher   ─┼─► mpsc channel ─► runtime::run ─► KeyResolver ─► Action
 tick           ─┘         ▲                                          │
                           │                                          ▼
                    effect results              App::update(&mut self, Action) -> Vec<Effect>
                           │                                          │
                           └────────── Effect executor ◄──────────────┘
                                       (storage::*, editor)
 after each batch of events: terminal.draw(|frame| ui::view(&app, frame))
```

- Events are read on a dedicated thread and sent over a `std::sync::mpsc` channel. RuDo has
  no async runtime; the work is a single interactive loop plus blocking file I/O.
- The loop blocks on the channel, drains all queued events, then draws once. There is no busy
  loop; it only wakes up without an event at the start of the next day, to update today's
  date, and a tick is only scheduled while a timed message is visible.
- The input thread polls with a short timeout and stops when its `InputThread` handle is
  dropped, so the terminal can be handed to an external editor and input resumed afterwards
  by spawning a new one.
- Only `KeyEventKind::Press` and `Repeat` are handled; `Release` events (Windows, kitty
  keyboard protocol) are dropped at the boundary.
- The runtime reports the terminal size with `App::resize` at start-up and on every resize
  event. `ui::layout` is the single source of the geometry: `view` draws into the areas it
  returns, and `App` derives the viewport height of a pane or overlay from the same functions
  (`ui::layout::pane_content`, `ui::layout::help_overlay`), so navigation moves by exactly
  the height that is drawn. There are no viewport constants. `App::resize` refreshes every
  outline with the new heights, so the selections stay visible, and `view` renders from the
  stored offsets without changing state.
- Text inputs scroll horizontally without stored state: `TextInputView` computes the scroll
  offset from the text, the cursor and the width of its row on every render (just far
  enough to show the cursor cell). A `TextInput` therefore holds only its text and cursor,
  `App` never needs to know the width of an input, and `view` renders inputs and
  `InputPopup`s from `&TextInput` and `&Prompt`. The price is that the cursor rests at the
  right edge while moving left through text scrolled to its end, instead of the text staying
  put until the cursor reaches the left edge.
- `ratatui::init` and `ratatui::restore` manage the terminal, including restoring it on panic.
  Use crossterm through `ratatui::crossterm` so there is exactly one crossterm version.

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
  between them. Moving, deleting, yanking and indenting always operate on whole blocks.
- The indent style is detected on demand (`indent_style(fallback)`): the first indented task
  decides between tab and spaces, the space width is the smallest step between a task and its
  parent; without indented tasks the configured `tasks.indent` is used.
- `TaskId` is assigned when a document is parsed and survives edits, so the UI keeps the
  selection on the same task while lines shift. Re-parsing (external reload) assigns new ids.
- Fenced code blocks and front matter are tracked while parsing; task-like lines inside them
  stay `Raw`.
- RuDo writes Dataview inline fields: `[due:: …]`, `[priority:: …]`, `[repeat:: …]`,
  `[completion:: …]`. A re-rendered task is written as
  `{indent}{marker} [{status}] {description}` followed by ` [key:: value]` for every field.

## Keymap model

- `Action` is a closed enum (`strum::EnumIter`, kebab-case names). Every action knows its
  description, the `Context`s it is valid in and its default key sequences, each through an
  exhaustive `match`, so adding an action fails to compile until all three are defined.
- `Context` is one of `Lists`, `Tasks`, `Help`, `Dialog`, `Input`. Global actions are valid in
  `Lists` and `Tasks`; navigation actions additionally in `Help`.
- The effective `Keymap` is built once at start-up from the defaults and the user's
  overrides and validated per context: no key sequence bound twice, no binding that is a prefix
  of another, no printable characters in `Input`. Validation errors name both actions.
- `KeyResolver` consumes one `KeyChord` at a time and answers `Action`, `Pending`,
  `Cancelled` (`esc` while a sequence is pending) or `NoMatch`. A chord that breaks a pending
  sequence is retried once as the start of a new one, and pending chords never carry over into
  another `Context`. Because prefixes are rejected at load time it needs no timeout.

## Persistence

- Features change documents only through `App::edit_list`, the single mutation choke point:
  it applies the edit and returns `Effect::Save` when the document changed. The executor
  writes atomically: a temporary file in the same directory, `fsync`, then `rename` over the
  target. Symlinks are resolved first so the link survives, and the original file permissions
  are kept.
- Before writing, the executor compares the file on disk with the content RuDo loaded. If it
  differs, RuDo does not overwrite it, reloads the file and reports that the last change was
  not applied.
- After saving, the content hash is remembered so the watcher can ignore RuDo's own writes.
- Operations on files and folders (create, rename, delete) are confirmed where destructive and
  are not part of the undo history; edits to tasks are.

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
| user scenarios                     | `tests/` drive a `TestApp` with key strings (`"o"`, `"Buy milk"`, `"enter"`) against a `TempDir` workspace and assert on the files |
| terminal and event loop            | kept too thin to need tests (humble object)                           |

Every test has the three phases required by AGENTS.md: setup, `system_under_test`, assertions.
Time is never read inside the core: functions that need today's date take it as a parameter,
and the shell passes `jiff::Zoned::now().date()`.

## Conventions

- No pictographic symbols in the interface. Box-drawing characters and simple geometric
  shapes (`▸`, `▾`) are fine; emoji and emoji-like dingbats are not.
- Logging goes to a file only (`$XDG_STATE_HOME/rudo/rudo.log`); writing to stdout or stderr
  would corrupt the terminal interface.
- Program output of command line subcommands (`rudo config default`) goes to stdout and is
  not logging.
- Dependencies are added with `cargo add`, with `default-features = false` where possible,
  and pass `cargo deny check`.
