"""Hammerhead — a minimax engine for HeXO, the hexagonal 6-in-a-row game.

Hammerhead plays HeXO, a two-stones-per-turn game on a hexagonal board.
This package is the in-process Python SDK: one :class:`Bot` drives one
game, advancing the position and answering queries about it.

Quickstart::

    from hammerhead import Bot

    bot = Bot(time_per_stone_ms=500)
    bot.play((0, 0))               # X opens at the origin
    while not bot.is_game_over:
        move = bot.suggest()       # engine picks the next stone
        bot.play(move)             # apply it
    print("winner:", bot.winner)

Moves are axial ``(q, r)`` coordinate tuples. ``Bot`` is stateful and
single-threaded — use one instance per game.

The full API reference, with worked examples, lives in ``docs/sdk.md``.
"""

from __future__ import annotations

from .bot import Bot
from .exceptions import (
    GameOverError,
    HammerheadError,
    IllegalMoveError,
    NotationError,
)
from .types import Move, Player

__version__ = "0.1.0"

__all__ = [
    "Bot",
    "Move",
    "Player",
    "HammerheadError",
    "IllegalMoveError",
    "GameOverError",
    "NotationError",
    "__version__",
]
