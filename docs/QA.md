# Manual QA checklist

The automated tests drive the application core (`tests/scenarios.rs` and the other files in
`tests/`) and the real binary in a pseudo-terminal (`just smoke`). What they cannot see is how
RuDo looks and feels in real terminals, next to real programs. Run this checklist before every
release, tick each item that behaves as described, and note anything that does not.

## Preparation

```sh
just check && just smoke
just install
rm -rf /tmp/rudo-qa && cp -r demo /tmp/rudo-qa
```

Record the environment:

- RuDo version (`rudo --version`):
- Operating system:
- Terminal emulator and version:
- tmux version:
- Neovim version:
- Obsidian version and Tasks plugin version:
- Date and tester:

Unless an item says otherwise, start RuDo with `rudo --root /tmp/rudo-qa`.

## Terminals

- [ ] **kitty or alacritty**: the interface draws without artefacts; borders, the fold
      markers `▸` `▾` and the scrollbar of the help screen use box-drawing characters, not
      replacement glyphs.
- [ ] The default theme follows the terminal colour scheme (try a light and a dark scheme).
- [ ] `q` and `ctrl-c` restore the terminal: the shell prompt is back, the cursor is visible,
      typed text echoes, and the scrollback from before RuDo is intact.
- [ ] **inside tmux**: the same items as above pass inside a tmux session.
- [ ] Inside tmux, `esc` closes prompts without a noticeable delay (with `escape-time` at its
      default and at `0`).
- [ ] Inside tmux, a hex colour from the theme looks right with the `RGB` terminal feature
      enabled, and the README warning describes what happens without it.

## Sizes and resizing

- [ ] At **80x24** both panes, the bottom bar with its key hints, and the help screen fit
      without cut-off words in the hints.
- [ ] In a **tiny terminal** (for example 20x5) RuDo says that the terminal is too small and
      does not panic; growing the terminal again brings the interface back.
- [ ] A narrow terminal shows only the focused pane with a hint, and `tab` switches panes.
- [ ] **Resizing** while a prompt, a dialog, the help screen or an inline edit is open
      redraws everything at once and keeps the typed text.
- [ ] Resizing quickly back and forth leaves no stale cells.

## Colours

- [ ] With **`NO_COLOR=1 rudo --root /tmp/rudo-qa`** no colours are used; the selection,
      the focused pane, done, cancelled and overdue tasks are still distinguishable through
      bold, dim, reversed and crossed-out text.
- [ ] With `NO_COLOR` set to an empty value colours are shown (the variable only counts when
      it is non-empty).

## Obsidian side by side

Open `/tmp/rudo-qa` as a vault (or a folder inside a vault) in Obsidian with the Tasks plugin
set to the Dataview task format, and RuDo next to it.

- [ ] Ticking a task in Obsidian appears in RuDo within a second, with a reload message.
- [ ] A task added, completed, given a due date and a priority in RuDo appears in Obsidian,
      and the Tasks plugin reads the due date and priority.
- [ ] A note with front matter, headings, prose and a code block keeps every line except the
      changed task lines (compare with `git diff --no-index` against a copy).
- [ ] Editing the same list in both programs at the same moment never loses the Obsidian
      edit; RuDo says that its last change was not applied.
- [ ] Creating, renaming and deleting a list in Obsidian updates the lists pane.
- [ ] The `.obsidian/` folder is not shown in the lists pane.

## Editor

- [ ] With `EDITOR=nvim`, **`e`** on a task opens Neovim at the line of that task, RuDo's
      interface is hidden while Neovim runs, and quitting Neovim brings RuDo back.
- [ ] Changes saved in Neovim are shown at once after it exits.
- [ ] `e` with an editor that does not exist reports an error in the bottom bar and keeps
      RuDo usable.
- [ ] `editor: code --wait` (or another command with arguments) in the configuration is used
      instead of `$EDITOR`.

## Configuration

Write each broken file to `/tmp/rudo-qa.yaml` and run
**`rudo config check --config /tmp/rudo-qa.yaml`**. Each must exit with a non-zero status
and report the problem with its line number; starting `rudo --config /tmp/rudo-qa.yaml`
must report the same problem instead of starting.

- [ ] Invalid YAML (for example an unclosed `[`).
- [ ] An unknown setting (`tasks: { show_complete: false }`).
- [ ] An unknown action in `keybindings` (`toggle-dne: x`).
- [ ] A malformed key (`toggle-done: ctrl-`).
- [ ] A key bound to two actions in the same pane (`toggle-done: o`).
- [ ] A binding that is a prefix of another one (`move-to-top: g`).
- [ ] An unknown theme slot or colour (`theme: { selection: { fg: purple } }`).
- [ ] `--config` pointing to a file that does not exist.
- [ ] A valid file reports success; `rudo config default` piped into a file passes the check.
- [ ] `rudo config path` prints the file in use, also with `XDG_CONFIG_HOME` set.
- [ ] A custom key from the configuration works and appears in the help screen (`?`) and in
      the bottom bar.

## Logs

- [ ] With `RUDO_LOG=debug`, `$XDG_STATE_HOME/rudo/rudo.log` receives log lines and nothing
      is written over the interface.

## Result

- Items that failed, with the issue opened for each:
- Notes:
