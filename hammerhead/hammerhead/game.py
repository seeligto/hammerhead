"""Game-record convenience.

Minimal for Phase 9. The CLI's ``selfplay`` loop demonstrates the full
two-engine pattern; helper here just records placements and the winner.
A richer driver lands in Phase 10 alongside the promotion harness.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Optional


Coord = tuple[int, int]


@dataclass(slots=True)
class GameRecord:
    """Linear record of placements + final winner."""

    moves: list[Coord] = field(default_factory=list)
    winner: Optional[int] = None  # 0 = X, 1 = O, None = ongoing / draw / unfinished

    def append(self, move: Coord) -> None:
        self.moves.append(move)

    def finish(self, winner: Optional[int]) -> None:
        self.winner = winner

    @property
    def ply(self) -> int:
        return len(self.moves)
