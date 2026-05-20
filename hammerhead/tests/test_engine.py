"""End-to-end smoke tests for the PyO3 ``Engine`` surface."""

from __future__ import annotations

import pytest

from hammerhead_engine import Engine


def test_initial_state() -> None:
    eng = Engine(tt_size_mb=4)
    assert eng.ply() == 0
    assert eng.halfmove() == 0
    assert eng.to_move() == 0  # X
    assert eng.winner() is None
    assert eng.cached_eval() == 0


def test_place_origin_then_query() -> None:
    eng = Engine(tt_size_mb=4)
    eng.place((0, 0))
    assert eng.ply() == 1
    assert eng.to_move() == 1  # O after X's singleton first
    assert eng.halfmove() == 0  # next turn starts fresh for O
    assert eng.winner() is None


def test_place_off_origin_first_raises() -> None:
    eng = Engine(tt_size_mb=4)
    with pytest.raises(ValueError):
        eng.place((1, 0))


def test_best_move_requires_budget() -> None:
    eng = Engine(tt_size_mb=4)
    eng.place((0, 0))
    with pytest.raises(ValueError):
        eng.best_move()


def test_best_move_on_empty_board_picks_origin() -> None:
    eng = Engine(tt_size_mb=4)
    assert eng.best_move(time_ms=100) == (0, 0)


def test_best_move_returns_legal_after_opening() -> None:
    eng = Engine(tt_size_mb=4)
    eng.place((0, 0))
    eng.place((1, 0))
    move = eng.best_move(time_ms=200)
    # legal range: within MAX_PIECE_DISTANCE of some piece, and empty
    assert isinstance(move, tuple)
    assert move not in {(0, 0), (1, 0)}


def test_find_pv_within_depth() -> None:
    eng = Engine(tt_size_mb=4)
    eng.place((0, 0))
    eng.best_move(time_ms=200)  # populate TT
    pv = eng.find_pv(4)
    assert isinstance(pv, list)
    assert len(pv) <= 4
    for coord in pv:
        assert isinstance(coord, tuple)
        assert len(coord) == 2


def test_find_pv_does_not_mutate_board() -> None:
    eng = Engine(tt_size_mb=4)
    eng.place((0, 0))
    eng.place((1, 0))
    eng.best_move(time_ms=200)
    ply_before = eng.ply()
    hash_before = eng.hash()
    eng.find_pv(6)
    assert eng.ply() == ply_before
    assert eng.hash() == hash_before


def test_reset_clears_state() -> None:
    eng = Engine(tt_size_mb=4)
    eng.place((0, 0))
    eng.place((1, 0))
    eng.reset()
    assert eng.ply() == 0
    assert eng.to_move() == 0
    assert eng.halfmove() == 0
    assert eng.winner() is None


def test_undo_round_trips() -> None:
    eng = Engine(tt_size_mb=4)
    eng.place((0, 0))
    h0 = eng.hash()
    eng.place((1, 0))
    eng.undo()
    assert eng.ply() == 1
    assert eng.hash() == h0


def test_clear_tt_keeps_position() -> None:
    eng = Engine(tt_size_mb=4)
    eng.place((0, 0))
    eng.best_move(time_ms=200)
    eng.clear_tt()
    assert eng.ply() == 1
    # TT empty ⇒ PV walk yields nothing.
    assert eng.find_pv(2) == []
    # The cleared TT must not break a follow-up search.
    eng.best_move(time_ms=100)


def test_halfmove_parity_through_o_pair() -> None:
    """X singleton, then O places both of their stones; halfmove tracks correctly."""
    eng = Engine(tt_size_mb=4)
    eng.place((0, 0))  # X singleton
    assert eng.to_move() == 1  # O
    assert eng.halfmove() == 0

    eng.place((1, 0))  # O stone 1
    assert eng.to_move() == 1  # O continues
    assert eng.halfmove() == 1

    eng.place((-1, 0))  # O stone 2
    assert eng.to_move() == 0  # back to X
    assert eng.halfmove() == 0


def test_best_move_returns_legal_move_under_tight_budget() -> None:
    """A 1ms budget exercises the depth-1 fallback path; the chosen move
    must still be placeable on a non-empty board."""
    eng = Engine(tt_size_mb=4)
    eng.place((0, 0))
    move = eng.best_move(time_ms=1)
    assert move != (0, 0), "engine must not return an already-occupied cell"
    eng.place(move)  # must succeed without raising
