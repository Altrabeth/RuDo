# Files, tasks and Obsidian

## How your files are organised

The **workspace** is a directory. RuDo shows its content as a tree in the lists pane, folders
first, then lists, each sorted by name ignoring case; `s d`, `s p`, `s o` and `s t` order them
by urgency, priority, open or total tasks instead, without changing any file (see
[Sorting](keybindings.md#sorting-lists-pane-and-tasks-pane)):

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
it is not empty, the `Bin` row at the end of the tree opens it (see [Bin](keybindings.md#bin-lists-pane)). `workspace.show_hidden: true` shows hidden entries from the start.

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

## Task syntax

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
| `[created:: YYYY-MM-DD]`    | creation date, written when RuDo creates a task              |
| `[anything:: value]`        | unknown fields are kept and written back unchanged           |

`x` marks any task that is not done as done, also an in-progress or cancelled one, and
writes `[completion:: <today>]` unless `tasks.record_completion_date` is off; marking a done
task open again removes its completion date. Only the selected task changes, never its
subtasks or parents.

Due dates are set with `D`. The prompt accepts `YYYY-MM-DD`, `today` (`tod`), `tomorrow`
(`tom`), `yesterday`, `+3d`, `+2w`, `+1m`, `+1y` (the `+` is optional), offsets into the past
such as `-3d` or `-1m`, and weekday names
(`fri`, `friday`: the next such day after today), and previews the date while you type; an
empty input or `none` removes the due date. `tab` completes these words (`fr` to `friday`, see
[completion](keybindings.md#picker-go-to-anything-and-folders)), in the recurrence prompt too.
The tasks pane shows `today`, `tomorrow`, `in 3d` for the next seven days, `2d overdue`, and
the date itself (`Dec 24`, or `2027-01-05` in another year) further ahead. A completed task
shows an overdue date as written.

A task without a priority has normal priority, between medium and low. `+` and `-` step
through `lowest`, `low`, normal, `medium`, `high` and `highest`; normal removes the field.
Priorities are shown at the right edge of the row: `!!!`, `!!` and `!` for highest to medium,
`-` and `--` for low and lowest; a value RuDo does not know is shown as written.

Recurrence rules are set with `r` and shown before the priority. Completing a recurring task
inserts its next occurrence above it: same text, tags, priority and fields, open, without a
completion date, due one interval after the old due date (or after today if it had none),
and created today like any new task (without a creation date if `tasks.created_date` is off).
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

Every task created in RuDo (`o`, `O` and `a` in the tasks pane, and the next occurrence of a
recurring task) gets today's date as `[created:: YYYY-MM-DD]`, the creation date of the
Obsidian Tasks plugin, unless `tasks.created_date` is off. It follows the fields typed into
the text, so a due date set later comes after it: `- [ ] Buy milk [created:: 2026-11-25]
[due:: 2026-11-27]`; a `created` field typed into the text is kept instead. Pasting is moving,
not creating: pasted tasks keep their fields as they are and never get a creation date. Tasks
without the field have no creation date; RuDo does not guess one from the file. The creation
date is not shown in the tasks pane and text search does not match it, but the query term
`created:` does, such as `created:>-1m` for tasks created in the last month (see
[Query terms](keybindings.md#query-terms)).

Lines inside fenced code blocks and front matter are never treated as tasks. Line endings
(LF or CRLF), a trailing newline and a byte order mark are preserved. Untouched lines are
written back byte for byte; only lines you edit in RuDo are re-rendered.

## Using RuDo with Obsidian

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

