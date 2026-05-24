"""The public :class:`Bot` — a clean, in-process handle on the engine.

This module is the canonical implementation of the SDK surface. Import
:class:`Bot` from the package root (``from hammerhead import Bot``); the
re-export there is the supported path.
"""

from __future__ import annotations

from typing import Any, Mapping, Optional

from hammerhead_engine import Engine

from .config import CONFIG
from .exceptions import GameOverError, IllegalMoveError, NotationError
from .types import Move, Player

# Engine reports the side to move as 0 / 1; the SDK speaks "X" / "O".
_SIDES: tuple[Player, Player] = ("X", "O")

# Default depth ceiling for the principal-variation walk. Bounds the
# query only — not an engine-tuning constant, so it lives here.
_DEFAULT_PV_DEPTH = 16

# Hard ceiling for the PV walk: the engine's find_pv takes a signed
# 8-bit depth, so a larger request would overflow at the boundary.
_MAX_PV_DEPTH = 127

MATE_SCORE: int = CONFIG.eval.mate_score
"""Score magnitude of a forced win.

A decisive position evaluates near ``±(MATE_SCORE - ply)`` — see
:meth:`Bot.evaluate`. Positive is a win for X, negative a win for O.
"""


class Bot:
    """High-level Hammerhead engine for embedding in Python projects.

    One ``Bot`` instance represents one game in progress. It is stateful:
    :meth:`play` advances the position, :meth:`suggest` and the other
    queries inspect it without mutating, :meth:`undo` rewinds one stone,
    and :meth:`reset` starts a fresh game.

    HeXO is a two-stones-per-turn game. ``play`` and ``suggest`` operate
    on a *single stone*; a normal turn is two stones for the same side.
    The one exception is X's opening, which is a single stone. Use
    :attr:`stone_in_turn` to tell which stone of the turn is next.

    ``Bot`` is **not thread-safe**. Use one instance per game and do not
    share it across threads. For parallel games, instantiate one ``Bot``
    per game.

    The engine is deterministic: identical move sequences and identical
    time budgets produce identical results. There is no random seed.

    Example:
        >>> from hammerhead import Bot
        >>> bot = Bot(time_per_stone_ms=200)
        >>> bot.play((0, 0))          # X opens at the origin
        >>> move = bot.suggest()      # ask the engine for O's reply
        >>> bot.play(move)            # apply it
        >>> bot.ply
        2

    Note:
        Some surface is planned but not yet available: string move
        notation (``play("A0")``, ``to_notation`` / ``from_notation``),
        per-side threat reports (``threats``), an ASCII board renderer
        (``board_ascii``), and live transposition-table resizing
        (``set_tt_size``). Moves are axial ``(q, r)`` coordinate tuples
        until the notation parsers ship.
    """

    def __init__(
        self,
        time_per_stone_ms: Optional[int] = None,
        tt_size_mb: Optional[int] = None,
    ) -> None:
        """Create a bot with an empty board.

        Args:
            time_per_stone_ms: Search budget per stone, in milliseconds.
                Applies to every :meth:`suggest` call that does not pass
                its own ``time_ms``. Defaults to the configured engine
                default (1000 ms) when ``None``.
            tt_size_mb: Transposition-table size in mebibytes. Larger
                tables help longer searches. Defaults to the configured
                engine default (64 MB) when ``None``.

        Raises:
            ValueError: if ``time_per_stone_ms`` or ``tt_size_mb`` is not
                positive.
        """
        budget = (
            CONFIG.bot.default_time_per_move_ms
            if time_per_stone_ms is None
            else time_per_stone_ms
        )
        if budget <= 0:
            raise ValueError("time_per_stone_ms must be positive")
        tt = CONFIG.bot.default_tt_size_mb if tt_size_mb is None else tt_size_mb
        if tt <= 0:
            raise ValueError("tt_size_mb must be positive")

        self._time_per_stone_ms: int = budget
        self._tt_size_mb: int = tt
        self._engine: Engine = Engine(tt_size_mb=tt)
        self._history: list[Move] = []

    def __repr__(self) -> str:
        return (
            f"Bot(ply={self.ply}, to_move={self.to_move!r}, "
            f"time_per_stone_ms={self._time_per_stone_ms}, "
            f"tt_size_mb={self._tt_size_mb})"
        )

    # ── State mutation ──────────────────────────────────────────────────

    def reset(self) -> None:
        """Reset to an empty board.

        Clears the move history and the engine position. Configuration —
        the time budget and table size — is preserved.
        """
        self._engine.reset()
        self._history.clear()

    def play(self, move: Move) -> None:
        """Apply one stone to the current position.

        Args:
            move: The stone to place, as an axial ``(q, r)`` coordinate
                tuple. X's opening stone is ``(0, 0)``.

        Raises:
            GameOverError: if the game has already been won.
            IllegalMoveError: if the move is not a legal placement — the
                cell is occupied, out of range, or (for the opening
                stone) not the origin ``(0, 0)``.
            NotationError: if ``move`` is a string — string notation is
                not supported yet; pass a ``(q, r)`` tuple.
            TypeError: if ``move`` is not a coordinate pair.

        Example:
            >>> bot = Bot()
            >>> bot.play((0, 0))
            >>> bot.history
            [(0, 0)]
        """
        if isinstance(move, str):
            raise NotationError(
                f"string notation is not supported yet: {move!r}; "
                "pass a (q, r) coordinate tuple"
            )
        coord = _coord(move)
        if self.is_game_over:
            raise GameOverError("game is over; no further stones may be played")
        try:
            self._engine.place(coord)
        except ValueError as exc:
            raise IllegalMoveError(f"cannot play {coord}: {exc}") from exc
        self._history.append(coord)

    def undo(self) -> None:
        """Undo the most recent stone.

        Removes one stone, not one turn. Call it twice to take back a
        full two-stone turn.

        Raises:
            IndexError: if no stones have been played.
        """
        if not self._history:
            raise IndexError("no stones to undo")
        self._engine.undo()
        self._history.pop()

    # ── Read-only state ─────────────────────────────────────────────────

    @property
    def to_move(self) -> Player:
        """The side that places the next stone — ``"X"`` or ``"O"``."""
        return _SIDES[self._engine.to_move()]

    @property
    def ply(self) -> int:
        """Total number of stones placed so far."""
        return self._engine.ply()

    @property
    def stone_in_turn(self) -> int:
        """Which stone of the current turn is next: ``0`` or ``1``.

        ``0`` means the next stone opens a turn; ``1`` means the side to
        move still owes the second stone of its turn. X's opening turn
        only ever reaches ``0``.
        """
        return self._engine.halfmove()

    @property
    def is_game_over(self) -> bool:
        """``True`` once a side has won, otherwise ``False``."""
        return self._engine.winner() is not None

    @property
    def winner(self) -> Optional[Player]:
        """The winning side (``"X"`` / ``"O"``), or ``None`` if undecided."""
        w = self._engine.winner()
        return None if w is None else _SIDES[w]

    @property
    def history(self) -> list[Move]:
        """The stones played so far, in order, as ``(q, r)`` tuples.

        Returns a fresh copy — mutating it does not affect the game.
        """
        return list(self._history)

    @property
    def time_per_stone_ms(self) -> int:
        """The default per-stone search budget, in milliseconds."""
        return self._time_per_stone_ms

    @property
    def tt_size_mb(self) -> int:
        """The transposition-table size, in mebibytes.

        Fixed at construction; resizing a live table is not yet
        supported.
        """
        return self._tt_size_mb

    # ── Engine queries (no state mutation) ──────────────────────────────

    def suggest(self, time_ms: Optional[int] = None) -> Move:
        """Return the engine's recommended next stone.

        Does not mutate the position — call :meth:`play` to apply the
        result.

        Args:
            time_ms: Search budget for this call only, in milliseconds.
                Falls back to the bot's configured per-stone budget when
                ``None``.

        Returns:
            The recommended stone as a ``(q, r)`` coordinate tuple.

        Raises:
            GameOverError: if the game has already been won.
            ValueError: if ``time_ms`` is not positive.

        Example:
            >>> bot = Bot(time_per_stone_ms=200)
            >>> bot.play((0, 0))
            >>> reply = bot.suggest()
            >>> bot.play(reply)
        """
        if self.is_game_over:
            raise GameOverError("game is over; there is no move to suggest")
        budget = self._time_per_stone_ms if time_ms is None else time_ms
        if budget <= 0:
            raise ValueError("time_ms must be positive")
        q, r = self._engine.best_move(time_ms=budget)
        return (q, r)

    def evaluate(self) -> int:
        """Return the static evaluation of the current position.

        Positive scores favour X, negative favour O — the engine is
        X-positive regardless of whose turn it is. A decisive position
        evaluates near ``±(MATE_SCORE - ply)`` — mate-for-X is large and
        positive, mate-for-O large and negative. :data:`MATE_SCORE` is
        importable from the package root for mate-detection logic.

        Returns:
            The signed evaluation in engine score units.
        """
        return self._engine.cached_eval()

    def principal_variation(self, max_depth: int = _DEFAULT_PV_DEPTH) -> list[Move]:
        """Return the engine's predicted best line from here.

        The line is read off the transposition table, so it is only
        meaningful after a search has populated it — call :meth:`suggest`
        first. The walk stops at the first table miss, so the result may
        be shorter than ``max_depth`` (and empty before any search).

        Args:
            max_depth: Maximum number of stones to walk. Values above
                127 are capped — the engine never reports a line that
                long anyway.

        Returns:
            The predicted stones, in order, as ``(q, r)`` tuples.

        Raises:
            ValueError: if ``max_depth`` is negative.
        """
        if max_depth < 0:
            raise ValueError("max_depth must be non-negative")
        depth = min(max_depth, _MAX_PV_DEPTH)
        return [(q, r) for q, r in self._engine.find_pv(depth)]

    # ── Configuration (mid-game safe) ───────────────────────────────────

    def set_time_per_stone(self, ms: int) -> None:
        """Change the default per-stone search budget.

        Safe to call mid-game; it takes effect on the next
        :meth:`suggest` that does not pass its own ``time_ms``.

        Args:
            ms: New per-stone budget in milliseconds.

        Raises:
            ValueError: if ``ms`` is not positive.
        """
        if ms <= 0:
            raise ValueError("ms must be positive")
        self._time_per_stone_ms = ms

    # ── Eval-weight overrides (Phase 28B-1) ─────────────────────────────

    def eval_overrides(self) -> dict[str, Any]:
        """Return the currently-active runtime eval overrides as a dict.

        Keys mirror the Rust ``EvalOverrides`` field names. Defaults
        equal the ``crate::config::*`` constants codegen'd from
        ``hexo.toml``.
        """
        return self._engine.eval_overrides()

    def set_eval_overrides(self, overrides: Mapping[str, Any]) -> None:
        """Patch the runtime eval overrides.

        Partial updates: keys absent from ``overrides`` retain their
        *current* value (not defaults — the call is incremental).
        Unknown keys raise ``ValueError``. Recognised keys:
        ``open_5``, ``closed_5``, ``open_4``, ``closed_4``,
        ``open_3``, ``closed_3``, ``open_2``,
        ``window_k_scores`` (sequence of 7 ints), ``open_extension_factor``,
        ``closed_extension_factor``, ``fork_cover2_bonus``.

        Passing an empty dict is a no-op — the call is byte-identical
        to never having been made (gate for the sweep driver's
        "no override applied" baseline).

        Persists across :meth:`reset`.

        Args:
            overrides: Mapping of override key → new value.
        """
        self._engine.set_eval_overrides(dict(overrides))


def _coord(move: Move) -> Move:
    """Normalise a move argument to a plain ``(int, int)`` tuple."""
    try:
        q, r = move
    except (TypeError, ValueError):
        raise TypeError(
            f"move must be a (q, r) coordinate pair, got {move!r}"
        ) from None
    return (int(q), int(r))
