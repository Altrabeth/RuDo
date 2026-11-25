# Configuration

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
[`assets/default-config.yaml`](../assets/default-config.yaml); `rudo config default` prints it.
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

## Keybindings

`keybindings` maps an action name (see the [keybinding tables](keybindings.md)) to one key or a
list of keys, written in the [key notation](keybindings.md#key-notation). The
keys you give *replace* the default keys of that action; actions you do not mention keep
their defaults, and an unbound action is still listed on the help screen. RuDo refuses a
configuration in which a key is bound to two actions in the same pane, in which one binding
is a prefix of another (`g` and `g g`), or in which `submit` or `cancel` starts with a key
that types text (a character without `ctrl` or `alt`), and lists every collision with the
actions involved.

## Theme

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

