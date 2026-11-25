#!/usr/bin/env python3
"""Smoke test of the real `rudo` binary in a pseudo-terminal.

Starts the binary on an empty workspace in a temporary folder, with every XDG directory
pointing into that folder, types the quick start of the README (create a list, open it, add
a task, complete it, quit) and checks the exit status, that the terminal was restored and
the Markdown left on disk.

Usage: scripts/smoke.py [path/to/rudo]   (default: target/debug/rudo)
"""

import datetime
import fcntl
import os
import pty
import re
import select
import struct
import sys
import tempfile
import termios
import time

COLUMNS, ROWS = 80, 24
STEP_TIMEOUT_SECONDS = 10.0
ENTER = b"\r"
LEAVE_ALTERNATE_SCREEN = b"\x1b[?1049l"
CURSOR_MOVE = re.compile(rb"\x1b\[[0-9;]*H")
OTHER_ESCAPE = re.compile(rb"\x1b\[[0-9;?]*[A-Za-z]")

# Each step sends its keys once the screen has shown the text the step waits for.
STEPS = [
    (b"No lists yet", b"o"),
    (b"New list", b"Inbox" + ENTER),
    (b"Inbox", ENTER),
    (b"No tasks yet", b"o"),
    (None, b"Buy milk" + ENTER),
    (b"Buy milk", b"x"),
    (b"[x]", b"q"),
]


def read_available(fd: int, timeout: float) -> bytes:
    """Returns what the terminal printed within `timeout` seconds, or b"" at its end."""
    ready, _, _ = select.select([fd], [], [], timeout)
    if not ready:
        return b""
    try:
        return os.read(fd, 65536)
    except OSError:
        return b""


def visible_text(output: bytes) -> bytes:
    """Returns the printed text of `output`, with each cursor move standing for one space.

    The terminal backend skips unchanged cells by moving the cursor instead of printing
    spaces, so words on one row arrive separated by cursor moves.
    """
    spaced = CURSOR_MOVE.sub(b" ", output)
    return re.sub(rb" +", b" ", OTHER_ESCAPE.sub(b"", spaced))


def wait_for(fd: int, text: bytes, output: bytearray) -> None:
    """Reads the terminal output into `output` until its visible text contains `text`."""
    deadline = time.monotonic() + STEP_TIMEOUT_SECONDS
    while text not in visible_text(output):
        if time.monotonic() > deadline:
            raise TimeoutError(f"the screen never showed {text!r}: {visible_text(output)!r}")
        output += read_available(fd, 0.1)


def run(binary: str, workspace: str, home: str) -> tuple[int, bytes]:
    """Drives `binary` through `STEPS`; returns its exit status and what it printed last."""
    environment = {
        "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
        "HOME": home,
        "TERM": "xterm-256color",
        "XDG_CONFIG_HOME": os.path.join(home, "config"),
        "XDG_DATA_HOME": os.path.join(home, "data"),
        "XDG_STATE_HOME": os.path.join(home, "state"),
    }
    pid, fd = pty.fork()
    if pid == 0:
        fcntl.ioctl(0, termios.TIOCSWINSZ, struct.pack("HHHH", ROWS, COLUMNS, 0, 0))
        os.execve(binary, [binary, "--root", workspace], environment)
    output = bytearray()
    for expected_text, keys in STEPS:
        if expected_text is not None:
            wait_for(fd, expected_text, output)
        output.clear()
        os.write(fd, keys)
        # Lets the binary see each step as separate key presses rather than a paste.
        time.sleep(0.2)
    deadline = time.monotonic() + STEP_TIMEOUT_SECONDS
    while time.monotonic() < deadline:
        output += read_available(fd, 0.1)
        finished, status = os.waitpid(pid, os.WNOHANG)
        if finished:
            while chunk := read_available(fd, 0.1):
                output += chunk
            return os.waitstatus_to_exitcode(status), bytes(output)
    os.kill(pid, 9)
    raise TimeoutError("rudo did not exit after q")


def main() -> int:
    binary = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else "target/debug/rudo")
    expected = f"- [x] Buy milk [completion:: {datetime.date.today().isoformat()}]\n"
    with tempfile.TemporaryDirectory(prefix="rudo-smoke-") as home:
        workspace = os.path.join(home, "workspace")
        os.mkdir(workspace)
        exit_code, last_output = run(binary, workspace, home)
        with open(os.path.join(workspace, "Inbox.md"), encoding="utf-8") as list_file:
            content = list_file.read()
    if exit_code != 0:
        print(f"smoke test failed: rudo exited with status {exit_code}", file=sys.stderr)
        return 1
    if LEAVE_ALTERNATE_SCREEN not in last_output:
        print("smoke test failed: rudo did not restore the terminal", file=sys.stderr)
        return 1
    if content != expected:
        print(f"smoke test failed: Inbox.md is {content!r}, expected {expected!r}", file=sys.stderr)
        return 1
    print("smoke test passed: rudo exited with status 0, restored the terminal and wrote Inbox.md")
    return 0


if __name__ == "__main__":
    sys.exit(main())
