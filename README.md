# RuDo

A keyboard-driven terminal UI for todo lists that live in plain Markdown files.

RuDo shows a folder of Markdown files as todo lists in a lazygit-style, multi-pane interface
with vim keybindings. Every list is an ordinary `.md` file and every task is an ordinary
Markdown checkbox, so you can point RuDo at a folder inside your Obsidian vault and work on
the same tasks from the terminal and from Obsidian at the same time.

```text
╭ Lists ─────────────────────╮╭ Groceries  1/4 ──────────────────────────────────────╮
│ ▾ Home                     ││ [ ] Buy milk #shopping                   due tomorrow │
│     Groceries         1/4  ││   [x] Check lactose-free                              │
│     Chores            0/2  ││ [ ] Bake bread                     !!       due in 3d │
│ ▸ Work                     ││ [ ] Water plants          every week     2d overdue   │
│   Inbox               0/1  ││                                                       │
╰────────────────────────────╯╰───────────────────────────────────────────────────────╯
 o new task  a subtask  i edit  x done  dd delete  D due date  ? help
```

> **Status:** under active development towards v1.0. The feature set below describes the v1.0
> target; progress is tracked in the [v1.0 roadmap](https://github.com/Altrabeth/RuDo/issues/1).

## Features

- **Plain Markdown storage.** Lists are `.md` files, folders group lists, tasks are GitHub
  flavoured task list items. Everything RuDo does not understand (front matter, headings,
  prose, code blocks) is preserved byte for byte.
- **Obsidian compatible.** Due dates, priorities and recurrence use the Dataview inline field
  format understood by the Obsidian Tasks plugin, and `#tags` are Obsidian tags.
- **Vim keybindings** for navigation (`j`, `k`, `gg`, `G`, `ctrl-d`, `ctrl-u`), editing
  (`o`, `O`, `i`, `dd`, `yy`, `p`, `>`, `<`), folding (`za`, `zM`, `zR`) and undo (`u`,
  `ctrl-r`).
- **Discoverable.** The bottom bar always shows the most important keys for the focused pane,
  empty panes tell you which key creates the first entry, and `?` opens a scrollable,
  searchable help screen generated from your *effective* keymap, so it always matches your
  configuration.
- **Nested tasks** with folding, reordering (`J`, `K`) and indenting (`>`, `<`).
- **Due dates** with relative input (`today`, `tomorrow`, `+3d`, `fri`, `2026-12-24`) and
  colour-coded relative display (`due in 3d`, `2d overdue`).
- **Priorities, tags and recurring tasks.** Completing a recurring task creates its next
  occurrence.
- **Search and filter** any pane with `/`, with vim-style smart case.
- **Yank and paste** tasks with their subtasks, also between lists.
- **Live reload.** Changes made by other programs (Obsidian, your editor, `git pull`) appear
  immediately; RuDo never silently overwrites them.
- **Open in your editor** at the selected task with `e`.
- **Configurable** colours and keybindings in `~/.config/rudo/config.yaml`.

## Installation

RuDo is built from source. You need a Rust toolchain installed with
[rustup](https://rustup.rs); the exact compiler version is pinned in `rust-toolchain.toml`
and installed automatically.

```sh
git clone git@github.com:Altrabeth/RuDo.git
cd RuDo
cargo install --path . --locked   # or: just install
```

This installs the `rudo` binary into `~/.cargo/bin`.

## Quick start

```sh
rudo --root ~/Obsidian/Vault/Todo
```

1. Press `o` to create your first list and type its name, then `enter`.
2. Press `enter` (or `l`) to open the list. The tasks pane gets the focus.
3. Press `o` to add a task, type it, press `enter`. Press `a` to add a subtask.
4. Press `x` to mark the selected task as done, `D` to give it a due date.
5. Press `?` at any time to see every available key.

Without `--root`, RuDo uses `workspace.root` from the configuration file, and falls back to
`~/.local/share/rudo` (more precisely `$XDG_DATA_HOME/rudo`), which is created on first start.

## How your files are organised

The **workspace** is a directory. RuDo shows its content as a tree in the lists pane:

| On disk                       | In RuDo                                          |
| ----------------------------- | ------------------------------------------------ |
| directory                     | folder, can be folded with `za`                  |
| `*.md` file                   | todo list, named after the file without `.md`    |
| hidden entries (`.obsidian/`) | ignored                                          |
| other files                   | ignored                                          |

A list file can contain anything. RuDo only touches task lines and keeps all other lines
exactly as they are:

```markdown
---
aliases: [shopping]
---

# Groceries

Things to pick up on Saturday.

- [ ] Buy milk #shopping [due:: 2026-11-25] [priority:: high]
	- [x] Check lactose-free [completion:: 2026-11-23]
- [ ] Bake bread [due:: 2026-11-27]
- [ ] Water plants [due:: 2026-11-22] [repeat:: every week]
- [/] Clean fridge
- [-] Buy candles
```

### Task syntax

| Markdown                    | Meaning                                                      |
| --------------------------- | ------------------------------------------------------------ |
| `- [ ] text`                | open task (`*`, `+` and `1.` list markers work as well)      |
| `- [x] text`, `- [X] text`  | done                                                         |
| `- [/] text`                | in progress (shown, toggling marks it done)                  |
| `- [-] text`                | cancelled (shown dimmed)                                     |
| indentation                 | subtask of the task above (tabs or spaces, detected per file)|
| `#tag`, `#area/subtag`      | Obsidian tag, highlighted                                    |
| `[due:: YYYY-MM-DD]`        | due date                                                     |
| `[priority:: high]`         | `highest`, `high`, `medium`, `low` or `lowest`               |
| `[repeat:: every 2 weeks]`  | recurrence: `every [N] day(s)/week(s)/month(s)/year(s)`      |
| `[completion:: YYYY-MM-DD]` | completion date, written when a task is marked done          |
| `[anything:: value]`        | unknown fields are kept and written back unchanged           |

A task without a priority has normal priority, between medium and low. Priorities are shown
at the right edge of the row: `!!!`, `!!` and `!` for highest to medium, `-` and `--` for low
and lowest; a value RuDo does not know is shown as written.

Lines inside fenced code blocks and front matter are never treated as tasks. Line endings
(LF or CRLF), a trailing newline and a byte order mark are preserved. Untouched lines are
written back byte for byte; only lines you edit in RuDo are re-rendered.

### Using RuDo with Obsidian

1. Point `workspace.root` at a folder inside your vault, for example `~/Vault/Todo`.
2. In the Obsidian Tasks plugin settings choose **Task Format: Dataview** so both programs
   read and write the same fields. Tasks written in the emoji format are preserved as text but
   their dates are not interpreted by RuDo.
3. Edit freely in both programs. RuDo watches the workspace and reloads changed files. If a
   file changes on disk while RuDo is about to save it, RuDo keeps the version on disk and
   tells you that your last change was not applied.

Renaming a list in RuDo renames the file but does not update `[[wikilinks]]` that point to
it; rename inside Obsidian if other notes link to the list.

## Keybindings

All keys can be changed in the configuration file. The help screen (`?`) always shows the
keys that are currently active. Key sequences such as `g g` are typed one key after the
other; the keys typed so far are shown in the bottom bar.

### Everywhere (lists and tasks pane)

| Keys            | Action             | Description                                         |
| --------------- | ------------------ | --------------------------------------------------- |
| `?`             | `help`             | open the help screen                                |
| `q`, `ctrl-c`   | `quit`             | quit RuDo                                           |
| `tab`           | `focus-next`       | focus the next pane                                 |
| `backtab`       | `focus-previous`   | focus the previous pane (`shift-tab`)               |
| `1`             | `focus-lists`      | focus the lists pane                                |
| `2`             | `focus-tasks`      | focus the tasks pane                                |
| `/`             | `search`           | filter the focused pane (also inside the help)      |
| `esc`           | `clear-filter`     | remove the active filter                            |
| `u`             | `undo`             | undo the last change to a task                      |
| `ctrl-r`        | `redo`             | redo the last undone change                         |
| `H`             | `toggle-completed` | show or hide completed tasks                        |
| `e`             | `open-in-editor`   | open the list in your editor at the selected task   |

### Moving around (lists pane, tasks pane and help screen)

| Keys                 | Action           | Description                    |
| -------------------- | ---------------- | ------------------------------ |
| `j`, `down`          | `move-down`      | next row                       |
| `k`, `up`            | `move-up`        | previous row                   |
| `g g`, `home`        | `move-to-top`    | first row                      |
| `G`, `end`           | `move-to-bottom` | last row                       |
| `ctrl-d`, `pagedown` | `half-page-down` | half a page down               |
| `ctrl-u`, `pageup`   | `half-page-up`   | half a page up                 |

### Trees (lists pane and tasks pane)

| Keys          | Action        | Description                                                  |
| ------------- | ------------- | ------------------------------------------------------------ |
| `h`, `left`   | `collapse`    | fold, else go to the parent; from a top-level task go to the lists pane |
| `l`, `right`  | `expand`      | unfold, else open the selected list                          |
| `z a`         | `toggle-fold` | fold or unfold the selected entry                            |
| `z M`         | `fold-all`    | fold everything                                              |
| `z R`         | `unfold-all`  | unfold everything                                            |

### Lists pane

| Keys    | Action       | Description                                               |
| ------- | ------------ | --------------------------------------------------------- |
| `enter` | `open`       | open the selected list, or fold or unfold a folder        |
| `o`     | `new-list`   | create a list next to the selection or inside the folder  |
| `O`     | `new-folder` | create a folder                                           |
| `r`     | `rename`     | rename the selected list or folder                        |
| `d d`   | `delete`     | delete the selected list or empty folder (asks first)     |

### Tasks pane

| Keys           | Action                | Description                                      |
| -------------- | --------------------- | ------------------------------------------------ |
| `o`            | `new-task-below`      | add a task below the selected one                |
| `O`            | `new-task-above`      | add a task above the selected one                |
| `a`            | `new-subtask`         | add a subtask to the selected task               |
| `i`, `enter`   | `edit`                | edit the task text                               |
| `x`, `space`   | `toggle-done`         | mark as done or open again                       |
| `d d`          | `delete`              | delete the task and its subtasks (yanks them)    |
| `y y`          | `yank`                | copy the task and its subtasks                   |
| `p`            | `paste-below`         | paste below the selected task                    |
| `P`            | `paste-above`         | paste above the selected task                    |
| `J`            | `move-task-down`      | swap with the next sibling                       |
| `K`            | `move-task-up`        | swap with the previous sibling                   |
| `>`            | `indent`              | make it a subtask of the task above              |
| `<`            | `outdent`             | move it one level up                             |
| `+`            | `priority-up`         | raise the priority                               |
| `-`            | `priority-down`       | lower the priority                               |
| `D`            | `set-due-date`        | set or clear the due date                        |
| `r`            | `set-recurrence`      | set or clear the recurrence                      |
| `s p`          | `sort-by-priority`    | sort the selected task and its siblings          |
| `s d`          | `sort-by-due-date`    | sort by due date, tasks without one last         |
| `s a`          | `sort-alphabetically` | sort by text                                     |
| `s s`          | `sort-by-status`      | open tasks first                                 |

While completed (done and cancelled) tasks are hidden with `H`, the pane title says
`completed hidden` and the counter still counts every task. An open subtask of a completed
task is hidden together with its parent.

### Help screen, dialogs and text input

| Keys                 | Action        | Where         | Description                 |
| -------------------- | ------------- | ------------- | --------------------------- |
| `?`, `q`, `esc`      | `close-help`  | help screen   | close the help screen       |
| `y`, `enter`         | `confirm-yes` | dialogs       | confirm                     |
| `n`, `esc`           | `confirm-no`  | dialogs       | cancel                      |
| `enter`              | `submit`      | text input    | accept the text             |
| `esc`, `ctrl-c`      | `cancel`      | text input    | discard the text            |

While typing, these editing keys are fixed: `left`, `right`, `ctrl-left` and `ctrl-right`
(word), `home` or `ctrl-a`, `end` or `ctrl-e`, `backspace`, `delete`, `ctrl-w` (delete the
word before the cursor), `ctrl-u` (delete to the start) and `ctrl-k` (delete to the end).

Vim users who expect `esc` to keep the typed text can bind `esc` to `submit` instead.

## Configuration

RuDo reads `$XDG_CONFIG_HOME/rudo/config.yaml`, which is `~/.config/rudo/config.yaml` unless
`XDG_CONFIG_HOME` is set (`config.yml` is accepted too). `--config <file>` selects another
file, which must exist. A missing default file is not an error; every setting has a default,
and a configuration file only needs the settings you want to change.

```sh
rudo config path      # print the configuration file RuDo uses
rudo config default   # print the complete default configuration with comments
rudo config check     # validate your configuration and report errors with line numbers
rudo config default > ~/.config/rudo/config.yaml   # start from the defaults
```

Unknown settings, actions, theme slots or malformed keys are errors, so typos are reported
instead of being ignored.

```yaml
workspace:
  # Folder holding your lists. `~` is expanded; a relative path is relative to this file.
  # Overridden by `--root`.
  root: ~/Obsidian/Vault/Todo

tasks:
  # Show completed tasks on start-up (toggle with `H`).
  show_completed: true
  # Write `[completion:: YYYY-MM-DD]` when a task is marked done.
  record_completion_date: true
  # Indentation for subtasks in files that have none yet: `tab` or a number of spaces.
  indent: tab

# Command used by `open-in-editor`. Defaults to $VISUAL, then $EDITOR, then `vi`.
# Arguments are allowed (`code --wait`). vi, vim, nvim, nano, emacs, kak, hx and micro open
# the list at the selected task; RuDo reloads the list when the editor exits.
editor: nvim

theme:
  selection: { fg: black, bg: cyan, modifiers: [bold] }
  due_overdue: { fg: "#ff5555" }
  tag: { fg: magenta }

keybindings:
  toggle-done: [x, space, ctrl-x]   # replaces the default keys of the action
  set-due-date: "t d"               # a key sequence
  open-in-editor: []                # unbinds the action
```

### Keybindings

`keybindings` maps an action name (see the tables above) to one key or a list of keys. The
keys you give *replace* the default keys of that action; actions you do not mention keep
their defaults. RuDo refuses a configuration in which a key is bound to two actions in the
same pane, in which one binding is a prefix of another (`g` and `g g`), or in which `submit`
or `cancel` starts with a key that types text (a character without `ctrl` or `alt`), and
lists every collision with the actions involved.

Key notation:

| Notation                                   | Key                                        |
| ------------------------------------------ | ------------------------------------------ |
| `a`, `A`, `?`, `>`, `1`                    | the character itself (`A` is shift-a)      |
| `ctrl-d`, `alt-x`, `ctrl-alt-x`            | with modifiers                             |
| `enter`, `esc`, `tab`, `backtab`, `space`  | named keys                                 |
| `backspace`, `delete`, `insert`            | named keys                                 |
| `up`, `down`, `left`, `right`              | arrow keys                                 |
| `home`, `end`, `pageup`, `pagedown`        | navigation keys                            |
| `f1` … `f12`                               | function keys                              |
| `g g`, `z a`                               | sequences, keys separated by a space       |

Modifiers can be combined in any order and are printed as `ctrl-alt-shift-`. Shift is part
of the character, so `shift-a` is the same key as `A` and `shift-?` the same as `?`;
`shift-tab` is `backtab`. Terminals send `ctrl-h` as `backspace`, `ctrl-i` as `tab`, `ctrl-m`
as `enter` and `ctrl-[` as `esc`, so these keys cannot be bound separately.

### Theme

`theme` maps a slot to a style. A style has an optional `fg` and `bg` colour and an optional
list of `modifiers` (`bold`, `dim`, `italic`, `underlined`, `reversed`, `crossed-out`). A slot
you set replaces the default style of that slot completely.

Colours are named ANSI colours (`black`, `red`, `green`, `yellow`, `blue`, `magenta`, `cyan`,
`gray`, `dark-gray`, `light-red`, `light-green`, `light-yellow`, `light-blue`,
`light-magenta`, `light-cyan`, `white`), `reset` for the terminal default, hex colours
(`"#rrggbb"`, quoted because `#` starts a YAML comment) or 256-colour indices (`"238"`). The
default theme only uses the 16 ANSI colours, so it follows your terminal's colour scheme. If
the `NO_COLOR` environment variable is set, colours are ignored and only modifiers are used.
Hex colours need a terminal with truecolour support; some setups, such as `tmux` without the
`RGB` terminal feature, show them wrongly, so prefer 256-colour indices there.

| Slot                                  | Used for                                         |
| ------------------------------------- | ------------------------------------------------ |
| `text`                                | default text                                     |
| `border`, `border_focused`            | pane borders                                     |
| `title`, `title_focused`              | pane titles                                      |
| `selection`, `selection_unfocused`    | selected row in the focused and the other pane   |
| `folder`, `list`, `list_count`        | lists pane entries and `done/total` counters     |
| `tree_guide`                          | indentation guides and fold markers              |
| `task_done`, `task_in_progress`, `task_cancelled` | task text by status                  |
| `priority_highest` … `priority_lowest`| priority markers (`highest`, `high`, `medium`, `low`, `lowest`) |
| `due`, `due_soon`, `due_today`, `due_overdue` | due dates                                |
| `tag`, `recurrence`                   | tags and recurrence rules                        |
| `status_bar`, `key_hint`, `key_hint_label`, `pending_keys` | bottom bar                  |
| `message_info`, `message_error`       | messages in the bottom bar                       |
| `popup_border`, `popup_title`, `input_error` | dialogs and text input                    |
| `help_heading`, `help_key`, `help_description` | help screen                             |
| `search_match`, `filter_context`      | search matches, and parents kept for context     |

## Command line

```text
rudo [OPTIONS] [COMMAND]

Commands:
  config path      Print the path of the configuration file in use
  config default   Print the default configuration
  config check     Validate the configuration file

Options:
  -r, --root <DIR>       Workspace folder (overrides `workspace.root`)
  -c, --config <FILE>    Configuration file (default: ~/.config/rudo/config.yaml)
  -h, --help             Print help
  -V, --version          Print version
```

## Troubleshooting

RuDo never writes log output to the terminal, because that would corrupt the interface. Logs
go to `$XDG_STATE_HOME/rudo/rudo.log` (default `~/.local/state/rudo/rudo.log`). Set the log
level with `RUDO_LOG`, which accepts
[`tracing` filter directives](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html):

```sh
RUDO_LOG=debug rudo
tail -f ~/.local/state/rudo/rudo.log
```

On Linux, very large workspaces can exceed the inotify watch limit. RuDo then keeps working
without live reload and shows `live reload disabled: ...`; raise `fs.inotify.max_user_watches`
to fix it.

## Development

The project uses [just](https://github.com/casey/just) as a command runner. Run `just` to list
all recipes.

| Recipe               | Purpose                                                        |
| -------------------- | -------------------------------------------------------------- |
| `just install-tools` | install `cargo-deny`, `cargo-insta` and `cargo-llvm-cov`       |
| `just install`       | install the `rudo` binary into `~/.cargo/bin`                  |
| `just run <args>`    | run RuDo from source, e.g. `just run --root /tmp/todo`         |
| `just demo`          | run RuDo on a fresh copy of the sample workspace in `demo/`    |
| `just test`          | run all tests including doctests                               |
| `just snap`          | review changed snapshots of the rendered interface             |
| `just lint`          | clippy with warnings denied                                    |
| `just cov`           | test coverage (`just cov --open` for the HTML report)          |
| `just check`         | everything that must pass before a commit                      |

Development is test driven: write a failing test, make it pass, refactor. The architecture
keeps the program logic in a pure core (state, update, rendering) that is tested without a
terminal, while a thin shell owns the terminal and the file system. See
[`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) for the module layout and the testing strategy,
and [`AGENTS.md`](AGENTS.md) for the coding rules every change must follow.

## Acknowledgements

RuDo is inspired by [dooit](https://github.com/dooit-org/dooit) and
[lazygit](https://github.com/jesseduffield/lazygit), and builds on
[ratatui](https://ratatui.rs).
