# Troubleshooting

## Logs

RuDo never writes log output to the terminal, because that would corrupt the interface. Logs
go to `$XDG_STATE_HOME/rudo/rudo.log` (default `~/.local/state/rudo/rudo.log`, on Windows
`%LOCALAPPDATA%\rudo\rudo.log`). The default
level is `info`; set another with `RUDO_LOG`, which accepts
[`tracing` filter directives](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html):

```sh
env RUDO_LOG=debug rudo                 # `env` works in every shell, including fish
tail -f ~/.local/state/rudo/rudo.log   # in a second terminal
```

## Configuration errors

If RuDo does not start because of the configuration, `rudo config check` shows every problem
with its line number.

## Live reload and the inotify limit

On Linux, very large workspaces can exceed the inotify watch limit. RuDo then keeps working
without live reload and shows `live reload disabled: ...`; raise `fs.inotify.max_user_watches`
to fix it.

## Terminal quirks

Terminals send `ctrl-h` as `backspace`, `ctrl-i` as `tab`, `ctrl-m` as `enter` and `ctrl-[` as
`esc`, so these keys cannot be bound separately (see [Key notation](keybindings.md#key-notation)).

Hex colours need a terminal with truecolour support; some setups, such as `tmux` without the
`RGB` terminal feature, show them wrongly, so prefer 256-colour indices there (see
[Theme](configuration.md#theme)).

Without a terminal on standard input and output, as in `docker run` without `-it`, `rudo`
exits at once with `rudo needs an interactive terminal` (see [Docker](platforms.md#docker)).
