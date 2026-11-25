# RuDo

A keyboard-driven terminal UI for todo lists that live in plain Markdown files.

RuDo shows a folder of Markdown files as todo lists in a lazygit-style, multi-pane interface
with vim keybindings. Every list is an ordinary `.md` file and every task is an ordinary
Markdown checkbox, so you can point RuDo at a folder inside your Obsidian vault and work on
the same tasks from the terminal and from Obsidian at the same time.

```text
╭ Lists ─────────────────╮╭ Groceries  1/4 ─────────────────────────────────────────────╮
│ ▾ Home                 ││ ▾ [ ] Buy milk #shopping                           tomorrow │
│     Chores         0/2 ││ │   [x] Check lactose-free                                  │
│   • Groceries      1/4 ││   [ ] Bake bread                                   !! in 3d │
│ ▾ Work                 ││   [ ] Water plants                    every week 2d overdue │
│     Inbox          0/1 ││                                                             │
╰────────────────────────╯╰─────────────────────────────────────────────────────────────╯
 o new task  a subtask  i edit  x done  dd delete  D due date                     ? help
```

> **Status:** release candidate for 1.0. Every feature below is implemented; the remaining
> 1.0 work is tracked in the [v1.0 milestone](https://github.com/Altrabeth/RuDo/milestone/1),
> changes are listed in the [changelog](CHANGELOG.md).

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
- **Nested tasks** with folding, reordering (`J`, `K`), indenting (`>`, `<`) and sorting
  (`s p`, `s d`, `s a`, `s s`).
- **Due dates** with relative input (`today`, `tomorrow`, `+3d`, `fri`, `2026-12-24`) and
  colour-coded relative display (`tomorrow`, `in 3d`, `2d overdue`).
- **Priorities, tags and recurring tasks.** Completing a recurring task creates its next
  occurrence.
- **Search and filter** any pane with `/`, with vim-style smart case.
- **Undo and redo** for every change to a task, across lists, and for creating, renaming,
  moving and deleting lists and folders.
- **A bin for deleted lists.** Deleted lists and folders go to `.trash/` in the workspace,
  the folder Obsidian uses for deleted notes, so a mistyped `dd` is never lost; browse,
  restore and empty it in the lists pane or with `rudo bin`.
- **Yank and paste** tasks with their subtasks, also between lists.
- **Live reload.** Changes made by other programs (Obsidian, your editor, `git pull`) appear
  immediately; RuDo never silently overwrites them.
- **Open in your editor** at the selected task with `e`.
- **Responsive layout.** Narrow terminals show only the focused pane.
- **Configurable** colours and keybindings in `~/.config/rudo/config.yaml`
  (`%APPDATA%\rudo\config.yaml` on Windows).

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
`~/.local/share/rudo` (more precisely `$XDG_DATA_HOME/rudo`; `%USERPROFILE%\Documents\RuDo` on
Windows), which is created on first start.
A folder given with `--root` or `workspace.root` must exist.

## How your files are organised

The **workspace** is a directory. RuDo shows its content as a tree in the lists pane, folders
first, then lists, each sorted by name ignoring case:

| On disk                         | In RuDo                                               |
| ------------------------------- | ----------------------------------------------------- |
| directory                       | folder, can be folded with `za`                       |
| `*.md` file                     | todo list, named after the file without `.md`         |
| hidden entries (`.obsidian/`, `.drafts.md`) | shown dimmed after `H` in the lists pane  |
| `.trash/` (the bin, `workspace.bin`) | deleted lists and folders; the `Bin` row at the end of the tree |
| other files                     | ignored                                               |
| symbolic link                   | a link to a `.md` file is a list; linked directories are not followed |

Folders and lists whose name starts with a dot are hidden until `H` in the lists pane shows
them; the pane title then says `hidden shown`, and `H` hides them again. Everything inside a
hidden folder is hidden with it, hidden folders that contain no `.md` file at any depth (such
as `.git/`) are left out completely, and the bin (`.trash/` at the top of the workspace, or
the folder configured as `workspace.bin`) is never part of the tree, not even after `H`; while
it is not empty, the `Bin` row at the end of the tree opens it (see [Bin](#bin-lists-pane)). `workspace.show_hidden: true` shows hidden entries from the start.

A list shows `done/total` for all its tasks, subtasks included; the active list, whose tasks
the tasks pane shows, is marked with `•`. A file that cannot be read or is not UTF-8 stays in
the tree, marked `unreadable`, and the tasks pane says why. Names of new lists and folders
must not start with a dot or contain any of `* " \ / < > : | ?`, which Obsidian rejects.
Deleting a list or folder asks first and moves it into the bin, keeping its path inside the
workspace (`Home/Groceries.md` goes to `.trash/Home/Groceries.md`); folders are moved with
everything inside them. An entry whose name is already in the bin is numbered
(`Groceries 2.md`), so nothing in the bin is replaced, and `.trash/.rudo-bin.json` remembers
the original path and the deletion time of every entry. `u` brings deleted entries back to
where they were, unless another entry took their place in the meantime. Lists and folders
can be moved into another folder with `m`, and several at once in visual mode (`v`).

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
| `- [ ] text`                | open task (`*`, `+`, `1.` and `1)` list markers work as well) |
| `- [x] text`, `- [X] text`  | done                                                         |
| `- [/] text`                | in progress                                                  |
| `- [-] text`                | cancelled (shown crossed out)                                |
| `- [?] text`                | any other status character is shown and kept as written      |
| indentation                 | subtask of the task above (tabs or spaces, detected per file)|
| `#tag`, `#area/subtag`      | Obsidian tag, highlighted                                    |
| `[due:: YYYY-MM-DD]`        | due date                                                     |
| `[priority:: high]`         | `highest`, `high`, `medium`, `low` or `lowest`               |
| `[repeat:: every 2 weeks]`  | recurrence: `every [N] day(s)/week(s)/month(s)/year(s)`      |
| `[completion:: YYYY-MM-DD]` | completion date, written when a task is marked done          |
| `[anything:: value]`        | unknown fields are kept and written back unchanged           |

`x` marks any task that is not done as done, also an in-progress or cancelled one, and
writes `[completion:: <today>]` unless `tasks.record_completion_date` is off; marking a done
task open again removes its completion date. Only the selected task changes, never its
subtasks or parents.

Due dates are set with `D`. The prompt accepts `YYYY-MM-DD`, `today` (`tod`), `tomorrow`
(`tom`), `yesterday`, `+3d`, `+2w`, `+1m`, `+1y` (the `+` is optional) and weekday names
(`fri`, `friday`: the next such day after today), and previews the date while you type; an
empty input or `none` removes the due date. The tasks pane shows `today`, `tomorrow`,
`in 3d` for the next seven days, `2d overdue`, and the date itself (`Dec 24`, or `2027-01-05`
in another year) further ahead. A completed task shows an overdue date as written.

A task without a priority has normal priority, between medium and low. `+` and `-` step
through `lowest`, `low`, normal, `medium`, `high` and `highest`; normal removes the field.
Priorities are shown at the right edge of the row: `!!!`, `!!` and `!` for highest to medium,
`-` and `--` for low and lowest; a value RuDo does not know is shown as written.

Recurrence rules are set with `r` and shown before the priority. Completing a recurring task
inserts its next occurrence above it: same text, tags, priority and fields, open, without a
completion date, due one interval after the old due date (or after today if it had none).
Months and years are added in calendar terms, so one month after January 31 is the end of
February. Subtasks are not copied, and one undo removes the completion and the new
occurrence. A rule RuDo does not support, such as `every weekday`, is shown as written;
completing such a task creates no new occurrence and says so.

New tasks are written as `- [ ] text`, with the list marker of the neighbouring task (the next
number below an ordered item) and, for a first subtask, one level of the file's indentation
(`tasks.indent` if the file has no indented task yet). Inline fields typed into the task text
become fields. A task RuDo changes is written as `{indent}{marker} [{status}] {description}`
followed by ` [key:: value]` for every field in their original order; setting a field
replaces it in place or appends it at the end. Pasted and moved tasks take over the
indentation of their new place.

Lines inside fenced code blocks and front matter are never treated as tasks. Line endings
(LF or CRLF), a trailing newline and a byte order mark are preserved. Untouched lines are
written back byte for byte; only lines you edit in RuDo are re-rendered.

### Using RuDo with Obsidian

1. Point `workspace.root` at a folder inside your vault, for example `~/Vault/Todo`.
2. In the Obsidian Tasks plugin settings choose **Task Format: Dataview** so both programs
   read and write the same fields. Tasks written in the emoji format are preserved as text but
   their dates are not interpreted by RuDo.
3. Edit freely in both programs. RuDo watches the workspace and reloads changed files, and
   new, renamed and deleted lists and folders appear in the lists pane. If a file changes on
   disk while RuDo is about to save it, RuDo keeps the version on disk, reloads it and tells
   you that your last change was not applied. A list reloaded from disk loses its undo
   history.
4. Lists and folders deleted in RuDo go to `.trash/` in the workspace. If the workspace is the
   vault itself, that is the folder Obsidian uses for deleted notes when **Deleted files** is
   set to **Move to Obsidian trash**, and RuDo tolerates the notes Obsidian puts there. The
   leading dot keeps the bin out of Obsidian's search, so tasks in deleted lists never show up
   in Tasks or Dataview queries. The bin is a plain folder: browse and empty it in RuDo, with
   `rudo bin list` and `rudo bin empty`, or by hand, or choose another one with
   `workspace.bin`. Notes Obsidian put there show up in the bin too; RuDo does not know where
   they came from and asks for a folder when you restore one.

Renaming or moving a list in RuDo renames or moves the file but does not update
`[[wikilinks]]` that point to it; rename or move it inside Obsidian if other notes link to the
list.

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
| `u`             | `undo`             | undo the last change to a task, list or folder      |
| `ctrl-r`        | `redo`             | redo the last undone change                         |
| `e`             | `open-in-editor`   | open the list in your editor at the selected task   |

`/` opens a search prompt in the bottom bar and filters the focused pane while you type,
with vim's smart case: a query without capital letters ignores case. The tasks pane shows
matching tasks with their parent tasks dimmed, ignoring folds; the lists pane shows matching
list names with their folders. `enter` keeps the filter (the pane title shows `/query`),
`esc` in the prompt or in the pane removes it. Every list has its own filter. Tasks you add
or edit while filtering stay visible; moving tasks with `J`, `K`, `>` and `<` asks you to
clear the filter first.

Undo and redo remember the last 100 changes to tasks in all lists and to lists and folders,
for as long as RuDo runs. Undoing a change in another list opens that list and selects the
changed task. Creating, renaming, moving and deleting lists and folders are undone like any
other change, a bulk operation of visual mode in one step: undoing a deletion moves the
entries back from the bin and selects them, undoing a new list moves it into the bin. If
another program took the original path in the meantime, the undo fails with a message and
the entry stays in the bin; `u` tries again once the path is free.

`e` opens the selected list in the editor from `editor`, `$VISUAL` or `$EDITOR` (in that
order, else `vi`, or `notepad` on Windows); in the tasks pane at the line of the selected
task. RuDo reloads the list when the editor exits.

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
| `m`     | `move-to-folder` | move the list, folder or selection into a folder      |
| `H`     | `toggle-hidden` | show or hide hidden folders and lists                    |
| `d d`   | `delete`     | move the selected list or folder to the bin (asks first)  |

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
| `H`            | `toggle-completed`    | show or hide completed tasks                     |

The task text is edited in its row; `enter` saves it, `esc` discards it, and empty text
changes nothing. Deleting a task does not ask: `u` brings it back and `p` pastes it. Every
action moves, deletes or copies a task together with its subtasks. Sorting is stable and
only sorts the siblings that no heading or text separates from the selected task; priority
sorts highest first, alphabetical sorting ignores case.

While completed (done and cancelled) tasks are hidden with `H`, the pane title says
`completed hidden` and the counter still counts every task. An open subtask of a completed
task is hidden together with its parent.

`H` in the lists pane shows and hides hidden folders and lists instead (`toggle-hidden`); a
binding of `toggle-completed` in your configuration only applies in the tasks pane.

`m` asks for the folder that receives the selected list or folder, relative to the workspace
folder (`Home/Garden`); empty text moves it to the top level. A folder cannot be moved into
itself, and an entry with the same name in the target folder is never replaced.

### Visual mode (lists pane, tasks pane and bin)

| Keys  | Action             | Description                                         |
| ----- | ------------------ | --------------------------------------------------- |
| `v`   | `visual-mode`      | select several rows from here on, or stop selecting |
| `d`   | `delete-selection` | delete the selected tasks, lists or folders         |
| `y`   | `yank-selection`   | copy the selected tasks and their subtasks          |
| `m`   | `move-to-folder`   | move the selected lists and folders into a folder   |

`v` starts selecting at the selected row, like vim's `V`. The keys of moving around (`j`, `k`,
`g g`, `G`, `ctrl-d`, `ctrl-u`) extend the selection, which always spans the rows shown
between where it started and the cursor; the bottom bar shows `-- VISUAL --` and how many rows
are selected. `v` again or `esc` stops selecting without changes, and so does every action on
the selection.

In the tasks pane `d` deletes the selected tasks with their subtasks in one step that `u`
undoes, after asking if more than one task is selected, and puts them into the register like
`d d`; `y` copies them. `p` and `P` paste everything in the register, so `v`, `d`, opening
another list and `p` moves tasks between lists. A folded task is selected with its hidden
subtasks. While a search filter is active only the shown matches are selected, not the tasks
hidden between them or the parents shown for context.

In the lists pane `d` moves the selected lists and folders to the bin after asking, and `m`
moves them into a folder; `u` undoes either in one step. If some files cannot be deleted or
moved, one message names them all.

In the bin `R` restores the selected entries and `d` deletes them permanently (see
[Bin](#bin-lists-pane)); `y` and `m` do nothing there.

### Bin (lists pane)

| Keys                 | Action             | Description                                                |
| -------------------- | ------------------ | ---------------------------------------------------------- |
| `R`                  | `restore-from-bin` | move the entry or the selection back to where it came from |
| `E`                  | `empty-bin`        | delete everything in the bin permanently (asks first)      |
| `h`, `left`, `esc`   | `close-bin`        | return to the lists                                        |

`enter` or `l` on the `Bin` row at the end of the tree opens the bin: the lists and folders in
it with their original path and the date they were deleted, the most recently deleted first.
The tasks pane shows the tasks of the selected list, read-only. `R` moves the entry back to its
original path; if another entry took that path in the meantime, RuDo says so and leaves the
entry in the bin, so rename or move the other entry first. Entries that another program, such
as Obsidian, put into the bin have no known original path, and `R` asks for the folder that
receives them. `u` undoes a restore like any other change.

`d d` in the bin deletes the selected entry permanently and `E` deletes everything in the bin
permanently, also files that are not lists; both ask first, saying what is deleted
(`Delete 12 lists with 340 tasks permanently? This cannot be undone.`), and cannot be undone.
These are the only ways RuDo deletes files for good. The moving around keys, `u`, `ctrl-r`,
`?` and `q` work in the bin as well.

`v` selects several entries of the bin, as in the panes (see
[Visual mode](#visual-mode-lists-pane-tasks-pane-and-bin)); the bottom bar shows
`-- VISUAL --` with the keys that apply. `R` then restores every selected entry to its
original path in one step that `u` undoes. Entries whose original path is taken stay in the
bin and selected, and one message names them (`restored 8 of 10; 2 paths are taken:
Home/Groceries.md, Work`); if some selected entries have no known original path, `R` asks
once for the folder that receives all of them. `d` deletes the selected entries permanently
after asking (`Delete 12 lists with 340 tasks and 2 folders permanently? This cannot be
undone.`); if some cannot be deleted, one message names them.

### Help screen, dialogs and text input

| Keys                 | Action        | Where         | Description                 |
| -------------------- | ------------- | ------------- | --------------------------- |
| `?`, `q`, `esc`      | `close-help`  | help screen   | close the help screen       |
| `y`, `enter`         | `confirm-yes` | dialogs       | confirm                     |
| `n`, `esc`           | `confirm-no`  | dialogs       | cancel                      |
| `enter`              | `submit`      | text input    | accept the text             |
| `esc`, `ctrl-c`      | `cancel`      | text input, visual mode (also in the bin) | discard the text, or end the selection |

While typing, these editing keys are fixed: `left`, `right`, `ctrl-left` and `ctrl-right`
(word), `home` or `ctrl-a`, `end` or `ctrl-e`, `backspace`, `delete`, `ctrl-w` (delete the
word before the cursor), `ctrl-u` (delete to the start) and `ctrl-k` (delete to the end).
Pasted text is inserted into an open text input and ignored everywhere else, so it never
triggers commands.

Vim users who expect `esc` to keep the typed text can bind `esc` to `submit` instead.

## Configuration

RuDo reads `$XDG_CONFIG_HOME/rudo/config.yaml`, which is `~/.config/rudo/config.yaml` unless
`XDG_CONFIG_HOME` is set, and `%APPDATA%\rudo\config.yaml` on Windows (`config.yml` is
accepted too). `--config <file>` selects another
file, which must exist. A missing default file is not an error; every setting has a default,
and a configuration file only needs the settings you want to change.

```sh
rudo config path      # print the configuration file RuDo uses
rudo config default   # print the complete default configuration with comments
rudo config check     # validate your configuration and report errors with line numbers
rudo config default > ~/.config/rudo/config.yaml   # start from the defaults
```

The complete default configuration, with a comment for every setting, is
[`assets/default-config.yaml`](assets/default-config.yaml); `rudo config default` prints it.
Unknown settings, actions, theme slots or malformed keys are errors, so typos are reported
instead of being ignored, and RuDo does not start with an invalid configuration. Booleans are
`true` or `false`, and a lone `~` or `null` means "not set". An example that changes a few
settings:

```yaml
workspace:
  # Folder holding your lists (default: null for $XDG_DATA_HOME/rudo). `~` is expanded; a
  # relative path is relative to this file. Overridden by `--root`.
  root: ~/Obsidian/Vault/Todo
  # Show hidden folders and lists, such as `.obsidian/`, on start-up (toggle with `H` in the
  # lists pane).
  show_hidden: false
  # Folder inside the workspace for deleted lists and folders (default: `.trash`).
  bin: .trash

tasks:
  # Show completed tasks on start-up (toggle with `H` in the tasks pane).
  show_completed: false
  # Write `[completion:: YYYY-MM-DD]` when a task is marked done.
  record_completion_date: true
  # Indentation for subtasks in files that have none yet: `tab` or 1 to 8 spaces.
  indent: 2

# Command used by `open-in-editor` (default: null for $VISUAL, then $EDITOR, then `vi`,
# or `notepad` on Windows).
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
their defaults, and an unbound action is still listed on the help screen. RuDo refuses a
configuration in which a key is bound to two actions in the same pane, in which one binding
is a prefix of another (`g` and `g g`), or in which `submit` or `cancel` starts with a key
that types text (a character without `ctrl` or `alt`), and lists every collision with the
actions involved.

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
you set replaces the default style of that slot completely; `{}` is plain text.

| Colour                                     | Meaning                                    |
| ------------------------------------------ | ------------------------------------------ |
| `black`, `red`, `green`, `yellow`, `blue`, `magenta`, `cyan`, `gray`, `dark-gray`, `light-red`, `light-green`, `light-yellow`, `light-blue`, `light-magenta`, `light-cyan`, `white` | the 16 ANSI colours of your terminal's colour scheme |
| `reset`                                    | the terminal's default colour              |
| `"#ff5555"`                                | hex colour, quoted because `#` starts a YAML comment |
| `238`                                      | 256-colour index from 0 to 255             |

The default theme only uses the 16 ANSI colours, so it follows your terminal's colour scheme.
If the `NO_COLOR` environment variable is set and not empty, colours are ignored and only
modifiers are used. Hex colours need a terminal with truecolour support; some setups, such as
`tmux` without the `RGB` terminal feature, show them wrongly, so prefer 256-colour indices
there.

| Slot                                  | Used for                                         |
| ------------------------------------- | ------------------------------------------------ |
| `text`                                | default text                                     |
| `border`, `border_focused`            | pane borders                                     |
| `title`, `title_focused`              | pane titles                                      |
| `selection`, `selection_unfocused`    | selected row in the focused and the other pane   |
| `visual_selection`                    | rows selected in visual mode                     |
| `folder`, `list`, `list_count`        | lists pane entries and `done/total` counters     |
| `hidden_entry`                        | hidden folders and lists while they are shown    |
| `bin`                                 | the bin at the end of the lists pane             |
| `tree_guide`                          | indentation guides and fold markers              |
| `task_done`, `task_in_progress`, `task_cancelled` | task text by status                  |
| `priority_highest`, `priority_high`, `priority_medium`, `priority_low`, `priority_lowest` | priority markers |
| `due`, `due_soon`, `due_today`, `due_overdue` | due dates: later, within seven days, today, past |
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
  bin list         List the lists and folders in the bin
  bin empty        Delete everything in the bin permanently

Options:
  -r, --root <DIR>       Workspace folder (overrides `workspace.root`)
  -c, --config <FILE>    Configuration file (default: see `rudo config path`)
  -h, --help             Print help
  -V, --version          Print version
```

`rudo config check` exits with status 1 and prints every problem if the configuration is
invalid.

`rudo bin list` prints one line per entry of the bin of the workspace (`--root` or
`workspace.root`), the most recently deleted first: the deletion time, the original path and,
if it differs, the path inside the bin; `unknown` for entries another program put there.
`rudo bin empty` asks on stdin before it deletes everything in the bin permanently, unless
`--yes` is given, and exits with status 1 if something could not be deleted.

## Troubleshooting

RuDo never writes log output to the terminal, because that would corrupt the interface. Logs
go to `$XDG_STATE_HOME/rudo/rudo.log` (default `~/.local/state/rudo/rudo.log`, on Windows
`%LOCALAPPDATA%\rudo\rudo.log`). The default
level is `info`; set another with `RUDO_LOG`, which accepts
[`tracing` filter directives](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html):

```sh
env RUDO_LOG=debug rudo                 # `env` works in every shell, including fish
tail -f ~/.local/state/rudo/rudo.log   # in a second terminal
```

If RuDo does not start because of the configuration, `rudo config check` shows every problem
with its line number.

On Linux, very large workspaces can exceed the inotify watch limit. RuDo then keeps working
without live reload and shows `live reload disabled: ...`; raise `fs.inotify.max_user_watches`
to fix it.

## Platforms

RuDo runs on Linux, macOS and Windows. The behaviour is the same everywhere except for these
locations and the last editor fallback:

| What               | Linux and macOS                | Windows                        |
| ------------------ | ------------------------------ | ------------------------------ |
| configuration file | `~/.config/rudo/config.yaml`   | `%APPDATA%\rudo\config.yaml`   |
| default workspace  | `~/.local/share/rudo`          | `%USERPROFILE%\Documents\RuDo` |
| log file           | `~/.local/state/rudo/rudo.log` | `%LOCALAPPDATA%\rudo\rudo.log` |
| editor fallback    | `vi`                           | `notepad`                      |

On every platform, `XDG_CONFIG_HOME`, `XDG_DATA_HOME` and `XDG_STATE_HOME` replace these
folders (with a `rudo` subfolder) when they hold an absolute path. If `%APPDATA%` or
`%LOCALAPPDATA%` is unset, RuDo uses `%USERPROFILE%\AppData\Roaming` or
`%USERPROFILE%\AppData\Local`. Paths inside the workspace are always shown with `/`, and the
index of the bin stores them with `/` too, so a workspace synced between Windows and Linux
reads the same; RuDo reads both `/` and `\`. New files are written with LF line endings on
every platform, as Obsidian does; existing files keep theirs.

## Development

The project uses [just](https://github.com/casey/just) as a command runner. Run `just` to list
all recipes.

| Recipe               | Purpose                                                        |
| -------------------- | -------------------------------------------------------------- |
| `just install-tools` | install `cargo-deny`, `cargo-insta` and `cargo-llvm-cov`       |
| `just build`         | build RuDo                                                     |
| `just install`       | install the `rudo` binary into `~/.cargo/bin`                  |
| `just run <args>`    | run RuDo from source, e.g. `just run --root /tmp/todo`         |
| `just demo`          | run RuDo on a fresh copy of the sample workspace in `demo/`    |
| `just fmt`           | format all code                                                |
| `just lint`          | clippy with warnings denied                                    |
| `just check-targets` | clippy for Windows and macOS from any system (installs the targets) |
| `just test`          | run all tests including doctests                               |
| `just smoke`         | run the binary in a pseudo-terminal through the quick start (needs `python3`) |
| `just snap`          | review changed snapshots of the rendered interface             |
| `just doc`           | build the API documentation with warnings denied               |
| `just cov`           | test coverage (`just cov --open` for the HTML report)          |
| `just deny`          | check dependencies for advisories and licenses                 |
| `just check`         | everything that must pass before a commit                      |

Development is test driven: write a failing test, make it pass, refactor. The architecture
keeps the program logic in a pure core (state, update, rendering) that is tested without a
terminal, while a thin shell owns the terminal and the file system. Guard tests compare the
keybinding, colour and theme tables of this README with the code, so they fail when one
changes without the other. See [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) for the module
layout and the testing strategy, [`docs/QA.md`](docs/QA.md) for the manual checklist in real
terminals before a release, and [`AGENTS.md`](AGENTS.md) for the coding rules every change
must follow.

CI runs `just check`, `just smoke` and the coverage on Linux for every push and pull request.
The lints and tests on Windows and macOS run when the workflow is started by hand and on
pushes to `main`. `.gitattributes` checks every text file out with LF line endings and the
fixtures in `tests/fixtures/` byte for byte, so the byte-exact tests mean the same on Windows.

## License

RuDo is free software: you can redistribute it and modify it under the terms of the
[GNU General Public License](LICENSE) as published by the Free Software Foundation, either
version 3 of the License or (at your option) any later version. Anyone may use, study, change
and share RuDo; software that is distributed on the basis of it must be released under the
same license, with its source code.

Copyright (C) 2026 the RuDo authors.

## Acknowledgements

RuDo is inspired by [dooit](https://github.com/dooit-org/dooit) and
[lazygit](https://github.com/jesseduffield/lazygit), and builds on
[ratatui](https://ratatui.rs).
