"""Subprocess-protocol integration test for ``hexo bot``."""

from __future__ import annotations

import subprocess
import sys
from typing import Optional

import pytest


def _spawn() -> subprocess.Popen[str]:
    return subprocess.Popen(
        [sys.executable, "-m", "hexo.cli", "bot", "--tt-size-mb", "4"],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        bufsize=1,
    )


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
    proc = _spawn()
    try:
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
    finally:
        if proc.poll() is None:
            proc.kill()


def test_subprocess_unknown_command() -> None:
    proc = _spawn()
    try:
        assert _wait_ready(proc) == "hexo bot ready"
        resp = _send(proc, "frobnicate")
        assert resp.startswith("error: unknown command")
        # Session must continue.
        assert _send(proc, "ply") == "0"
        _send(proc, "quit")
        proc.wait(timeout=5)
    finally:
        if proc.poll() is None:
            proc.kill()


def test_subprocess_error_is_surfaced() -> None:
    proc = _spawn()
    try:
        assert _wait_ready(proc) == "hexo bot ready"
        # First move must be at origin.
        resp = _send(proc, "place 5 5")
        assert resp.startswith("error:")
        # Session continues.
        assert _send(proc, "place 0 0") == "ok"
        _send(proc, "quit")
        proc.wait(timeout=5)
    finally:
        if proc.poll() is None:
            proc.kill()


@pytest.mark.parametrize("malformed", ["place", "place 1", "best_move", "best_move foo"])
def test_subprocess_malformed_commands_emit_error(malformed: str) -> None:
    proc = _spawn()
    try:
        assert _wait_ready(proc) == "hexo bot ready"
        resp = _send(proc, malformed)
        assert resp.startswith("error:")
        _send(proc, "quit")
        proc.wait(timeout=5)
    finally:
        if proc.poll() is None:
            proc.kill()
