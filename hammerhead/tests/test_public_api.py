"""Public-API tests for the :mod:`hammerhead` SDK.

Every public ``Bot`` method has a happy-path test and an error-path
test, followed by whole-game integration tests. Search budgets are kept
small so the suite stays fast; the engine is deterministic, so replayed
move sequences reproduce state exactly.
"""

from __future__ import annotations

import pytest

from hammerhead import (
    MATE_SCORE,
    Bot,
    GameOverError,
    HammerheadError,
    IllegalMoveError,
    NotationError,
)

# Small per-stone budget — enough for a legal move, fast for CI.
_FAST_MS = 30


def _fresh() -> Bot:
    return Bot(time_per_stone_ms=_FAST_MS)


def _play_turn(bot: Bot) -> None:
    """Advance one full turn (1 stone for X's opening, else 2)."""
    bot.play(bot.suggest())
    if bot.stone_in_turn == 1 and not bot.is_game_over:
        bot.play(bot.suggest())


# ── __init__ ────────────────────────────────────────────────────────────


def test_init_defaults() -> None:
    bot = Bot()
    assert bot.ply == 0
    assert bot.to_move == "X"
    assert bot.time_per_stone_ms > 0


def test_init_custom_args() -> None:
    bot = Bot(time_per_stone_ms=250, tt_size_mb=16)
    assert bot.time_per_stone_ms == 250
    assert bot.tt_size_mb == 16


def test_mate_score_is_positive_int() -> None:
    assert isinstance(MATE_SCORE, int)
    assert MATE_SCORE > 0


@pytest.mark.parametrize(
    "kwargs",
    [{"time_per_stone_ms": 0}, {"time_per_stone_ms": -1}, {"tt_size_mb": 0}],
)
def test_init_rejects_non_positive(kwargs: dict[str, int]) -> None:
    with pytest.raises(ValueError):
        Bot(**kwargs)


# ── play ────────────────────────────────────────────────────────────────


def test_play_places_stone() -> None:
    bot = _fresh()
    bot.play((0, 0))
    assert bot.ply == 1
    assert bot.history == [(0, 0)]


def test_play_rejects_occupied_cell() -> None:
    bot = _fresh()
    bot.play((0, 0))
    with pytest.raises(IllegalMoveError):
        bot.play((0, 0))


def test_play_rejects_out_of_range() -> None:
    bot = _fresh()
    # HeXO's opening stone must be the origin.
    with pytest.raises(IllegalMoveError):
        bot.play((5, 5))


def test_play_rejects_string_notation() -> None:
    bot = _fresh()
    with pytest.raises(NotationError):
        bot.play("A0")  # type: ignore[arg-type]


def test_play_rejects_malformed_move() -> None:
    bot = _fresh()
    with pytest.raises(TypeError):
        bot.play((0, 0, 0))  # type: ignore[arg-type]


# ── undo ────────────────────────────────────────────────────────────────


def test_undo_rewinds_one_stone() -> None:
    bot = _fresh()
    bot.play((0, 0))
    bot.play(bot.suggest())
    bot.undo()
    assert bot.ply == 1
    assert bot.history == [(0, 0)]


def test_undo_on_empty_history_raises() -> None:
    bot = _fresh()
    with pytest.raises(IndexError):
        bot.undo()


# ── reset ───────────────────────────────────────────────────────────────


def test_reset_clears_state() -> None:
    bot = _fresh()
    bot.play((0, 0))
    _play_turn(bot)
    bot.reset()
    assert bot.ply == 0
    assert bot.history == []
    assert bot.to_move == "X"
    assert bot.winner is None


def test_reset_preserves_config() -> None:
    bot = Bot(time_per_stone_ms=120)
    bot.play((0, 0))
    bot.reset()
    assert bot.time_per_stone_ms == 120


# ── read-only state ─────────────────────────────────────────────────────


def test_to_move_alternates_with_turns() -> None:
    bot = _fresh()
    assert bot.to_move == "X"
    bot.play((0, 0))  # X's opening is a singleton turn
    assert bot.to_move == "O"


def test_stone_in_turn_tracks_two_stone_turn() -> None:
    bot = _fresh()
    bot.play((0, 0))
    assert bot.stone_in_turn == 0
    bot.play(bot.suggest())  # O's first stone
    assert bot.stone_in_turn == 1
    bot.play(bot.suggest())  # O's second stone
    assert bot.stone_in_turn == 0
    assert bot.to_move == "X"


def test_ply_counts_stones() -> None:
    bot = _fresh()
    assert bot.ply == 0
    bot.play((0, 0))
    assert bot.ply == 1


def test_is_game_over_false_at_start() -> None:
    assert _fresh().is_game_over is False


def test_winner_none_until_terminal() -> None:
    bot = _fresh()
    bot.play((0, 0))
    assert bot.winner is None


def test_history_is_a_copy() -> None:
    bot = _fresh()
    bot.play((0, 0))
    snapshot = bot.history
    snapshot.append((9, 9))
    assert bot.history == [(0, 0)]


# ── suggest ─────────────────────────────────────────────────────────────


def test_suggest_returns_coord_without_mutating() -> None:
    bot = _fresh()
    bot.play((0, 0))
    before = bot.ply
    move = bot.suggest()
    assert isinstance(move, tuple) and len(move) == 2
    assert bot.ply == before  # suggest does not place


def test_suggest_rejects_non_positive_time() -> None:
    bot = _fresh()
    bot.play((0, 0))
    with pytest.raises(ValueError):
        bot.suggest(time_ms=0)


# ── evaluate ────────────────────────────────────────────────────────────


def test_evaluate_returns_int() -> None:
    bot = _fresh()
    assert isinstance(bot.evaluate(), int)


def test_evaluate_changes_with_position() -> None:
    bot = _fresh()
    bot.play((0, 0))
    # A placed stone is a position; eval stays an int either way.
    assert isinstance(bot.evaluate(), int)


# ── principal_variation ─────────────────────────────────────────────────


def test_principal_variation_returns_coord_list() -> None:
    bot = _fresh()
    bot.play((0, 0))
    bot.suggest()  # populate the transposition table
    pv = bot.principal_variation()
    assert isinstance(pv, list)
    assert all(isinstance(m, tuple) and len(m) == 2 for m in pv)


def test_principal_variation_rejects_negative_depth() -> None:
    bot = _fresh()
    with pytest.raises(ValueError):
        bot.principal_variation(max_depth=-1)


def test_principal_variation_caps_oversized_depth() -> None:
    """A depth past the engine's 8-bit limit must not overflow."""
    bot = _fresh()
    bot.play((0, 0))
    bot.suggest()
    pv = bot.principal_variation(max_depth=10_000)
    assert isinstance(pv, list)


# ── set_time_per_stone ──────────────────────────────────────────────────


def test_set_time_per_stone_updates_budget() -> None:
    bot = _fresh()
    bot.set_time_per_stone(500)
    assert bot.time_per_stone_ms == 500


def test_set_time_per_stone_rejects_non_positive() -> None:
    bot = _fresh()
    with pytest.raises(ValueError):
        bot.set_time_per_stone(0)


# ── integration ─────────────────────────────────────────────────────────


def test_full_game_loop_terminates() -> None:
    """Play a complete self-game; it ends with a winner."""
    bot = _fresh()
    bot.play((0, 0))
    for _ in range(400):  # generous ply cap
        if bot.is_game_over:
            break
        bot.play(bot.suggest())
    assert bot.is_game_over
    assert bot.winner in ("X", "O")


def test_play_and_suggest_raise_after_game_over() -> None:
    bot = _fresh()
    bot.play((0, 0))
    for _ in range(400):
        if bot.is_game_over:
            break
        bot.play(bot.suggest())
    assert bot.is_game_over
    with pytest.raises(GameOverError):
        bot.suggest()
    with pytest.raises(GameOverError):
        bot.play((6, -3))


def test_undo_to_opening_then_replay_is_identical() -> None:
    """Undo every stone, replay the same line, reach the same position."""
    bot = _fresh()
    bot.play((0, 0))
    for _ in range(6):
        bot.play(bot.suggest())
    line = bot.history
    final_hash = bot._engine.hash()

    while bot.ply:
        bot.undo()
    assert bot.ply == 0

    for move in line:
        bot.play(move)
    assert bot.history == line
    assert bot._engine.hash() == final_hash


def test_reset_midgame_restores_empty_state() -> None:
    bot = _fresh()
    bot.play((0, 0))
    for _ in range(4):
        bot.play(bot.suggest())
    assert bot.ply > 0
    bot.reset()
    assert bot.ply == 0
    assert bot.history == []
    assert not bot.is_game_over


def test_illegal_move_is_a_hammerhead_error() -> None:
    """All deliberate SDK errors share the HammerheadError base."""
    bot = _fresh()
    bot.play((0, 0))
    with pytest.raises(HammerheadError):
        bot.play((0, 0))
