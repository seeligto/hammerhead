"""Tests for the :class:`SearchStats` opt-in telemetry surface and the
fixed-depth ``Bot.suggest(depth=N)`` mode.

Backwards-compat: default ``Bot.suggest(time_ms=T)`` must keep returning
a plain ``(q, r)`` tuple — existing callers see no behavioural change.
"""

from __future__ import annotations

import dataclasses

import pytest

from hammerhead import Bot, SearchStats

# Small budget — fast for CI, deterministic per-engine.
_FAST_MS = 30


def _fresh() -> Bot:
    return Bot(time_per_stone_ms=_FAST_MS)


# ── Backwards-compat default ───────────────────────────────────────────


def test_suggest_default_returns_plain_coord() -> None:
    """Default ``return_stats=False`` keeps the legacy single-Move return."""
    bot = _fresh()
    bot.play((0, 0))
    move = bot.suggest(time_ms=_FAST_MS)
    assert isinstance(move, tuple)
    assert len(move) == 2
    assert all(isinstance(x, int) for x in move)


# ── Opt-in SearchStats shape ───────────────────────────────────────────


def test_suggest_return_stats_shape() -> None:
    bot = _fresh()
    bot.play((0, 0))
    result = bot.suggest(time_ms=_FAST_MS, return_stats=True)
    assert isinstance(result, tuple) and len(result) == 2
    move, stats = result
    assert isinstance(move, tuple) and len(move) == 2
    assert isinstance(stats, SearchStats)
    assert isinstance(stats.max_depth_reached, int)
    assert isinstance(stats.nodes, int)
    assert isinstance(stats.nps, float)
    assert isinstance(stats.time_ms, float)
    assert isinstance(stats.score, int)


def test_search_stats_non_zero_on_real_search() -> None:
    bot = _fresh()
    bot.play((0, 0))
    _, stats = bot.suggest(time_ms=_FAST_MS, return_stats=True)
    assert stats.nodes > 0
    assert stats.max_depth_reached >= 1
    assert stats.time_ms > 0.0
    assert stats.nps > 0.0


def test_search_stats_nps_matches_nodes_per_second() -> None:
    bot = _fresh()
    bot.play((0, 0))
    _, stats = bot.suggest(time_ms=_FAST_MS, return_stats=True)
    expected = stats.nodes / (stats.time_ms / 1000.0)
    # Float equality within a wide tolerance — SDK computes from the
    # same two values, no rounding intervenes beyond float arithmetic.
    assert abs(stats.nps - expected) < 1.0


def test_search_stats_is_frozen() -> None:
    bot = _fresh()
    bot.play((0, 0))
    _, stats = bot.suggest(time_ms=_FAST_MS, return_stats=True)
    with pytest.raises(dataclasses.FrozenInstanceError):
        stats.nodes = 0  # type: ignore[misc]


# ── Fixed-depth surface ────────────────────────────────────────────────


def test_suggest_depth_only_succeeds() -> None:
    bot = _fresh()
    bot.play((0, 0))
    move, stats = bot.suggest(depth=2, return_stats=True)
    assert isinstance(move, tuple) and len(move) == 2
    assert stats.max_depth_reached >= 2


def test_suggest_depth_no_time_cap() -> None:
    """``depth=N`` alone must not silently honour the construction-time
    budget — the depth-only contract gives the search as long as it needs.
    """
    bot = Bot(time_per_stone_ms=1)
    bot.play((0, 0))
    # depth=2 + 1 ms construction budget would never complete depth 2
    # if the construction default leaked through.
    _, stats = bot.suggest(depth=2, return_stats=True)
    assert stats.max_depth_reached >= 2


def test_suggest_depth_validation() -> None:
    bot = _fresh()
    bot.play((0, 0))
    with pytest.raises(ValueError):
        bot.suggest(depth=0)
    with pytest.raises(ValueError):
        bot.suggest(depth=-1)


def test_suggest_both_bounds_set_returns_normally() -> None:
    """``time_ms`` + ``depth`` is permissive — search aborts on first hit."""
    bot = _fresh()
    bot.play((0, 0))
    move = bot.suggest(time_ms=1, depth=8)
    assert isinstance(move, tuple) and len(move) == 2


def test_suggest_depth_is_deterministic_across_time_settings() -> None:
    """Same fixed-depth call from the same position yields the same move
    irrespective of ``time_ms`` (engine is deterministic; depth cap binds).
    """
    bot_a = _fresh()
    bot_a.play((0, 0))
    move_a, stats_a = bot_a.suggest(depth=2, return_stats=True)

    bot_b = _fresh()
    bot_b.play((0, 0))
    move_b, stats_b = bot_b.suggest(
        time_ms=10_000, depth=2, return_stats=True,
    )
    assert move_a == move_b
    assert stats_a.max_depth_reached == stats_b.max_depth_reached
    assert stats_a.nodes == stats_b.nodes


# ── Docstring snapshot (Section C of the design) ───────────────────────


def test_suggest_docstring_clarifies_per_stone() -> None:
    doc = Bot.suggest.__doc__ or ""
    assert "Per-stone" in doc or "per-stone" in doc or "per stone" in doc


def test_init_docstring_clarifies_per_stone() -> None:
    doc = Bot.__init__.__doc__ or ""
    assert "per stone" in doc or "per-stone" in doc.lower()
