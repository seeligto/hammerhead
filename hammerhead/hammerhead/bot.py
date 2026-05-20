"""High-level Bot wrapping the Rust ``Engine``.

One bot owns one engine. ``play_turn`` plays 1 or 2 stones depending on
the X-singleton rule (X's first stone is a singleton turn; every other
turn is two stones for the same side).
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Optional

from hammerhead_engine import Engine

from .config import CONFIG


Coord = tuple[int, int]


@dataclass(frozen=True, slots=True)
class BotConfig:
    time_per_move_ms: int = CONFIG.bot.default_time_per_move_ms
    max_depth: Optional[int] = None
    tt_size_mb: int = CONFIG.bot.default_tt_size_mb


class Bot:
    """Convenience wrapper over a single ``Engine``."""

    def __init__(self, cfg: BotConfig = BotConfig()) -> None:
        self.cfg = cfg
        self.engine: Engine = Engine(tt_size_mb=cfg.tt_size_mb)

    def reset(self) -> None:
        self.engine.reset()

    def play_stone(self) -> Coord:
        """Search one stone, place it, return the coord."""
        kwargs: dict[str, int] = {"time_ms": self.cfg.time_per_move_ms}
        if self.cfg.max_depth is not None:
            kwargs["depth"] = self.cfg.max_depth
        move = self.engine.best_move(**kwargs)
        self.engine.place(move)
        return move

    def play_turn(self) -> list[Coord]:
        """Play 1 or 2 stones; return the list of coords placed.

        Returns ``[]`` if the game is already terminal. The same-side
        continuation is driven by ``halfmove``: after the first stone, a
        halfmove of ``1`` means the same side still has stone 2 to play.
        """
        moves: list[Coord] = []
        if self.engine.winner() is not None:
            return moves
        moves.append(self.play_stone())
        if self.engine.winner() is None and self.engine.halfmove() == 1:
            moves.append(self.play_stone())
        return moves

    def observe(self, move: Coord) -> None:
        """Apply an externally-played stone to the local engine."""
        self.engine.place(move)

    def winner(self) -> Optional[int]:
        return self.engine.winner()

    def halfmove(self) -> int:
        return self.engine.halfmove()

    def to_move(self) -> int:
        return self.engine.to_move()
