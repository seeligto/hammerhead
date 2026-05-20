"""Subprocess-protocol integration test for ``hexo bot``."""

from __future__ import annotations

import subprocess
import sys
from contextlib import contextmanager
from typing import Iterator, Optional

import pytest


@contextmanager
def _bot_proc() -> Iterator[subprocess.Popen[str]]:
    """Spawn a ``hexo bot`` subprocess and guarantee its pipes close.

    Plain ``Popen.__exit__`` waits then drops references, but the pipe
    objects themselves only close on GC — pytest with ``-W error`` then
    promotes the resulting :class:`ResourceWarning` to a failure. Close
    each pipe explicitly here.
    """
    proc = subprocess.Popen(
        [sys.executable, "-m", "hammerhead.cli", "bot", "--tt-size-mb", "4"],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        bufsize=1,
    )
    try:
        yield proc
    finally:
        if proc.poll() is None:
            proc.kill()
            proc.wait(timeout=5)
        for fd in (proc.stdin, proc.stdout, proc.stderr):
            if fd is not None and not fd.closed:
                fd.close()


def _send(proc: subprocess.Popen[str], cmd: str) -> str:
    assert proc.stdin is not None
    assert proc.stdout is not None
    proc.stdin.write(cmd + "\n")
    proc.stdin.flush()
    return proc.stdout.readline().rstrip("\n")


def _wait_ready(proc: subprocess.Popen[str]) -> str:
    assert proc.stdout is not None
    return proc.stdout.readline().rstrip("\n")


def test_subprocess_protocol_round_trip() -> None:
    with _bot_proc() as proc:
        assert _wait_ready(proc) == "hexo bot ready"
        assert _send(proc, "reset") == "ok"
        assert _send(proc, "ply") == "0"
        assert _send(proc, "to_move") == "X"
        assert _send(proc, "halfmove") == "0"
        assert _send(proc, "winner") == "none"

        assert _send(proc, "place 0 0") == "ok"
        assert _send(proc, "ply") == "1"
        assert _send(proc, "to_move") == "O"

        h_line = _send(proc, "hash")
        assert len(h_line) == 32
        int(h_line, 16)  # well-formed hex

        eval_line = _send(proc, "eval")
        int(eval_line)  # integer-castable

        bm = _send(proc, "best_move 150")
        q_str, r_str = bm.split()
        q, r = int(q_str), int(r_str)
        # placeable by following with the placement
        assert _send(proc, f"place {q} {r}") == "ok"

        assert _send(proc, "quit") == "bye"
        rc: Optional[int] = proc.wait(timeout=5)
        assert rc == 0


def test_subprocess_unknown_command() -> None:
    with _bot_proc() as proc:
        assert _wait_ready(proc) == "hexo bot ready"
        resp = _send(proc, "frobnicate")
        assert resp.startswith("error: unknown command")
        # Session must continue.
        assert _send(proc, "ply") == "0"
        _send(proc, "quit")
        proc.wait(timeout=5)


def test_subprocess_error_is_surfaced() -> None:
    with _bot_proc() as proc:
        assert _wait_ready(proc) == "hexo bot ready"
        # First move must be at origin.
        resp = _send(proc, "place 5 5")
        assert resp.startswith("error:")
        # Session continues.
        assert _send(proc, "place 0 0") == "ok"
        _send(proc, "quit")
        proc.wait(timeout=5)


@pytest.mark.parametrize("malformed", ["place", "place 1", "best_move", "best_move foo"])
def test_subprocess_malformed_commands_emit_error(malformed: str) -> None:
    with _bot_proc() as proc:
        assert _wait_ready(proc) == "hexo bot ready"
        resp = _send(proc, malformed)
        assert resp.startswith("error:")
        _send(proc, "quit")
        proc.wait(timeout=5)
