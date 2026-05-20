"""Public type aliases for the :mod:`hammerhead` SDK.

These names exist so consumer code and type checkers can speak the SDK's
vocabulary without reaching into engine internals.
"""

from __future__ import annotations

from typing import Literal, Tuple

Move = Tuple[int, int]
"""A single stone, expressed as an axial hex coordinate ``(q, r)``.

HeXO is played on a hexagonal board addressed by axial coordinates: ``q``
is the column axis, ``r`` the row axis, both centred on the origin
``(0, 0)`` where X opens. ``Move`` is the only move representation the
SDK accepts today. String notation (BKE / BSN / HXN) is planned; when it
lands, :meth:`Bot.play` will additionally accept ``str``.
"""

Player = Literal["X", "O"]
"""A side to move. ``"X"`` opens the game; ``"O"`` replies.

The engine is X-positive: a positive evaluation favours ``"X"``.
"""
