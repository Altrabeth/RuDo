# Keybindings

All keys can be changed in the [configuration file](configuration.md#keybindings). The help
screen (`?`) always shows the keys that are currently active. Key sequences such as `g g` are
typed one key after the other; the keys typed so far are shown in the bottom bar.

## Everywhere (lists and tasks pane)

| Keys            | Action             | Description                                         |
| --------------- | ------------------ | --------------------------------------------------- |
| `?`             | `help`             | open the help screen                                |
| `q`, `ctrl-c`   | `quit`             | quit RuDo                                           |
| `tab`           | `focus-next`       | focus the next pane                                 |
| `backtab`       | `focus-previous`   | focus the previous pane (`shift-tab`)               |
| `1`             | `focus-lists`      | focus the lists pane                                |
| `2`             | `focus-tasks`      | focus the tasks pane                                |
| `/`             | `search`           | filter the focused pane (also inside the help)      |
| `ctrl-p`        | `go-to`            | go to any folder, list or task of the workspace (fuzzy finder) |
| `esc`           | `clear-filter`     | remove the active filter                            |
| `u`             | `undo`             | undo the last change to a task, list or folder      |
| `ctrl-r`        | `redo`             | redo the last undone change                         |
| `e`             | `open-in-editor`   | open the list in your editor at the selected task   |

`/` opens a search prompt in the bottom bar and filters the focused pane while you type,
with vim's smart case: a word without capital letters ignores case. Every word must match,
in any order, and [query terms](#query-terms) such as `due:today` or `#home` match tasks by
their dates and fields. The tasks pane shows
matching tasks with their parent tasks dimmed, ignoring folds; the lists pane shows matching
list names with their folders. `enter` keeps the filter (the pane title shows `/query`),
`esc` in the prompt or in the pane removes it. Every list has its own filter. Tasks you add
or edit while filtering stay visible; moving tasks with `J`, `K`, `>` and `<` asks you to
clear the filter first.

`ctrl-p` opens go to anything: a [picker](#picker-go-to-anything-and-folders) over every
folder, list and task of the workspace, unlike `/`, which filters only the focused pane. Each
match shows what it is (`folder`, `list`, `task`) and a task the list it belongs to
(`Buy milk · Home/Groceries`); completed tasks are dimmed. Tasks match by their text, folders and
lists by their path with matches in the name first, so `home` lists the folder `Home` and the
task `Clean the home office` before the lists inside `Home/`; on equal scores folders and
lists come before tasks and open tasks before completed ones. `enter` on a folder selects it
in the lists pane, on a list opens it, and on a task opens its list and selects the task;
folded parents are unfolded, and a task hidden by the search filter is revealed, one hidden
with `H` shows the completed tasks again. `esc` closes the finder without moving. Hidden
folders and lists are searched while `H` shows them; the bin and lists that cannot be read
are not searched. Inside the picker `ctrl-p` selects the previous match (`picker-previous`);
the two bindings live in different contexts and do not conflict. To open the finder with
`ctrl-space` instead, bind `go-to: ctrl-space` in the configuration (macOS uses `ctrl-space`
to switch the keyboard layout, and tmux often uses it as its prefix, so it is not the
default).

Undo and redo remember the last 100 changes to tasks in all lists and to lists and folders,
for as long as RuDo runs. Undoing a change in another list opens that list and selects the
changed task. Creating, renaming, moving and deleting lists and folders are undone like any
other change, a bulk operation of visual mode in one step: undoing a deletion moves the
entries back from the bin and selects them, undoing a new list moves it into the bin. If
another program took the original path in the meantime, the undo fails with a message and
the entry stays in the bin; `u` tries again once the path is free.

`e` opens the selected list in the editor from `editor`, `$VISUAL` or `$EDITOR` (in that
order, else `vi`, or `notepad` on Windows; see [Platforms](platforms.md) for how the
command is split); in the tasks pane at the line of the selected task. RuDo reloads the list when the editor exits.

## Moving around (lists pane, tasks pane and help screen)

| Keys                 | Action           | Description                    |
| -------------------- | ---------------- | ------------------------------ |
| `j`, `down`          | `move-down`      | next row                       |
| `k`, `up`            | `move-up`        | previous row                   |
| `g g`, `home`        | `move-to-top`    | first row                      |
| `G`, `end`           | `move-to-bottom` | last row                       |
| `ctrl-d`, `pagedown` | `half-page-down` | half a page down               |
| `ctrl-u`, `pageup`   | `half-page-up`   | half a page up                 |

## Trees (lists pane and tasks pane)

| Keys          | Action        | Description                                                  |
| ------------- | ------------- | ------------------------------------------------------------ |
| `h`, `left`   | `collapse`    | fold, else go to the parent; from a top-level task go to the lists pane |
| `l`, `right`  | `expand`      | unfold, else open the selected list                          |
| `z a`         | `toggle-fold` | fold or unfold the selected entry                            |
| `z M`         | `fold-all`    | fold everything                                              |
| `z R`         | `unfold-all`  | unfold everything                                            |

## Lists pane

| Keys    | Action       | Description                                               |
| ------- | ------------ | --------------------------------------------------------- |
| `enter` | `open`       | open the selected list, or fold or unfold a folder        |
| `o`     | `new-list`   | create a list next to the selection or inside the folder  |
| `O`     | `new-folder` | create a folder                                           |
| `r`     | `rename`     | rename the selected list or folder                        |
| `m`     | `move-to-folder` | move the list, folder or selection into a folder      |
| `H`     | `toggle-hidden` | show or hide hidden folders and lists                    |
| `d d`   | `delete`     | move the selected list or folder to the bin (asks first)  |

## Tasks pane

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
| `H`            | `toggle-completed`    | show or hide completed tasks                     |

The task text is edited in its row; `enter` saves it, `esc` discards it, and empty text
changes nothing. A new task gets today's creation date (see
[Task syntax](files-and-obsidian.md#task-syntax)), a pasted one keeps its fields. Deleting a task does not ask: `u` brings it back and `p` pastes it. Every
action moves, deletes or copies a task together with its subtasks.

While completed (done and cancelled) tasks are hidden with `H`, the pane title says
`completed hidden` and the counter still counts every task. An open subtask of a completed
task is hidden together with its parent.

`H` in the lists pane shows and hides hidden folders and lists instead (`toggle-hidden`); a
binding of `toggle-completed` in your configuration only applies in the tasks pane.

`m` asks for the folder that receives the selected list or folder in the
[picker](#picker-go-to-anything-and-folders): type a few letters of the folder, such as `gar`
for `Home/Garden`, and press `enter`; `/` is the top level of the workspace. A folder cannot
be moved into itself, and an entry with the same name in the target folder is never replaced.

## Sorting (lists pane and tasks pane)

| Keys  | Action                | Description                                        |
| ----- | --------------------- | -------------------------------------------------- |
| `s p` | `sort-by-priority`    | sort by priority, highest first                    |
| `s d` | `sort-by-due-date`    | sort by due date, earliest first, without one last |
| `s a` | `sort-alphabetically` | sort by text or name                               |
| `s s` | `sort-by-status`      | sort tasks: open tasks first                       |
| `s o` | `sort-by-open-tasks`  | sort lists: most open tasks first                  |
| `s t` | `sort-by-total-tasks` | sort lists: most tasks first                       |

In the tasks pane, sorting changes the list file: it sorts the selected task and its
siblings, together with their subtasks. Sorting is stable and only sorts the siblings that no
heading or text separates from the selected task; priority sorts highest first, alphabetical
sorting ignores case.

In the lists pane, sorting is a view that never changes a file: `s a`, `s d`, `s p`, `s o`
and `s t` order the folders and lists of every folder, the pane title names the order
(`Lists by due`), and the order stays for the session, also when lists are edited, reloaded
or added. `lists.sort` in the [configuration](configuration.md) sets the order at start.
Only open tasks, those neither done nor cancelled, count for urgency and priority:

- `s a` (`name`, the default): by name, ignoring case.
- `s d` (`due`): most urgent first, by the earliest due date of the open tasks, so overdue
  lists come first; lists without an open task with a due date follow.
- `s p` (`priority`): by the highest priority of the open tasks, a task without a priority
  counting as normal (between medium and low); equal priorities by urgency as with `s d`;
  lists without open tasks follow.
- `s o` (`open-tasks`) and `s t` (`total-tasks`): most open tasks, or most tasks, first.

Folders always come before lists and are ordered among themselves by everything the pane
shows below them: the sum of the counts, the earliest due date and the highest priority. Equal
entries are ordered by name, lists that cannot be read come last (except by name), hidden
entries sort like the others and the `Bin` row stays at the end. The selection, folded
folders and a visual selection stay with their entries when the order changes.

## Visual mode (lists pane, tasks pane and bin)

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

## Bin (lists pane)

| Keys                 | Action             | Description                                                |
| -------------------- | ------------------ | ---------------------------------------------------------- |
| `R`                  | `restore-from-bin` | move the entry or the selection back to where it came from |
| `E`                  | `empty-bin`        | delete everything in the bin permanently (asks first)      |
| `h`, `left`, `esc`   | `close-bin`        | return to the lists                                        |

`enter` or `l` on the `Bin` row at the end of the tree opens the bin: the lists and folders
in it with their original path and the date they were deleted, the most recently deleted
first. The tasks pane shows the tasks of the selected list, read-only. `R` moves the entry
back to its original path; if another entry took that path in the meantime, RuDo says so and
leaves the entry in the bin, so rename or move the other entry first. Entries that another
program, such as Obsidian, put into the bin have no known original path, and `R` asks for the
folder that receives them in the [picker](#picker-go-to-anything-and-folders). `u` undoes a
restore like any other change.

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

## Help screen, dialogs and text input

| Keys                 | Action        | Where         | Description                 |
| -------------------- | ------------- | ------------- | --------------------------- |
| `?`, `q`, `esc`      | `close-help`  | help screen   | close the help screen       |
| `y`, `enter`         | `confirm-yes` | dialogs       | confirm                     |
| `n`, `esc`           | `confirm-no`  | dialogs       | cancel                      |
| `enter`              | `submit`      | text input, picker | accept the text        |
| `esc`, `ctrl-c`      | `cancel`      | text input, picker, visual mode (also in the bin) | discard the text, or end the selection |

While typing, these editing keys are fixed: `left`, `right`, `ctrl-left` and `ctrl-right`
(word), `home` or `ctrl-a`, `end` or `ctrl-e`, `backspace`, `delete`, `ctrl-w` (delete the
word before the cursor), `ctrl-u` (delete to the start) and `ctrl-k` (delete to the end).
Pasted text is inserted into an open text input and ignored everywhere else, so it never
triggers commands.

Vim users who expect `esc` to keep the typed text can bind `esc` to `submit` instead.

## Picker (go to anything and folders)

| Keys             | Action            | Description                                                          |
| ---------------- | ----------------- | -------------------------------------------------------------------- |
| `down`, `ctrl-n` | `picker-next`     | select the next match                                                |
| `up`, `ctrl-p`   | `picker-previous` | select the previous match                                            |
| `tab`            | `complete`        | complete the text or the word at the cursor; again for the next one  |

Go to anything (`ctrl-p`, see [above](#everywhere-lists-and-tasks-pane)) uses the same picker
for folders, lists and tasks. Wherever RuDo asks for a folder (`m`, and `R` in the bin for
entries of unknown origin), it opens a picker like fzf: the folders of the workspace that
match the typed text are listed below it, the best match first, with the matched letters
highlighted and the position in the bottom border (`1-10/42`). The letters only need to
appear in order (`hgar` finds `Home/Garden`), matches in the folder name rank before matches
in its parents, and case follows the smart case of `/`. `/` stands for the top level of the
workspace; hidden folders are listed while `H` shows them, the bin never. `tab` puts the
selected folder into the input and, pressed again, the next one. `enter` takes the selected
folder, or the typed path if nothing matches, which reports a folder that does not exist
(create it with `O` first); `esc` cancels. The
[editing keys](#help-screen-dialogs-and-text-input) work in the input.

`complete` also works in the due date (`D`) and recurrence (`r`) prompts: `tab` completes
the word before the cursor to a word the prompt understands (`fr` to `friday`, `ev` to
`every`), ignoring case, and keeps the text after the cursor. If several words fit, the hint
below the input lists them and each `tab` shows the next one, then the typed text again,
while the preview shows the date or rule of the word shown; typing or moving the cursor accepts
it. Without a fitting word nothing changes. The recurrence prompt offers only the words valid
at their position: `every`, then a unit (`day`, `week`, `month`, `year`), or its plural after
an interval (`every 2 w` to `every 2 weeks`). In the search prompt (`/`) `tab` completes
[query terms](#query-terms) such as `due:to` to `due:today`, listing the candidates after the
query. The prompts for names ignore `tab`.

## Query terms

The search (`/`) of both panes and go to anything (`ctrl-p`) find tasks by their dates and
fields, not only by their text. A query is a list of words separated by spaces, all of which
must match. A word with one of the prefixes below is a term; every other word is text, also a
word with an unknown prefix such as `foo:bar`:

| Terms                 | Matches                                         |
| --------------------- | ----------------------------------------------- |
| `due:today`           | due on that day (today, fri, +3d, -1w, 2026-12-01) |
| `due:<=fri`           | due before or after that day (<, <=, >, >=)     |
| `due:overdue`         | open and due before today                       |
| `due:none`, `due:any` | without or with a due date                      |
| `created:>-1m`        | created in the last month (dates as for due)    |
| `done:<=-1w`          | completed a week ago or earlier                 |
| `priority:>=medium`   | priority (lowest, low, medium, high, highest)   |
| `#tag`                | with the tag or a tag nested in it              |
| `is:open`, `is:done`  | open, or done or cancelled                      |

Dates are the expressions of the due date prompt (`D`) relative to today, including offsets
into the past such as `-3d`, `-1w`, `-1m` and `-1y`; a weekday is the next one after today. A task
whose field holds no `YYYY-MM-DD` date has no such date. `created:` and `done:` take the same
values as `due:` but `overdue`, so `created:none` finds tasks without a creation date. A task
without a priority, or with an unknown one, has normal priority, which lies between `low` and
`medium`: `priority:>low` includes it. Tags ignore case, and `#home` also finds `#home/garden`.
Term values ignore case; prefixes are lower case.
Smart case applies to the text words only, each on its own, so `milk Buy` finds `Buy oat
milk`. A term with an invalid value, such as `due:blah`, shows the error next to the query
and matches nothing until it is fixed.

- In the tasks pane, the matching tasks are shown with their parent tasks, as for text.
- In the lists pane, a query with at least one term shows the lists that contain a matching
  task, completed ones included, with their folders and the number of matches before the
  `done/total` counter; text words still match list names. `due:today is:open` lists where
  something is due today.
- In go to anything, a query with at least one term offers only tasks; the text words rank
  them as before.

## Key notation

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
