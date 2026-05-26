"""Sprint 4A — Python-surface tests for runtime search params.

Covers:
- ``Bot.search_params`` getter returns the codegen'd defaults,
- partial-dict ``set_search_params`` patches only the specified keys,
- empty-dict is a no-op,
- override survives :meth:`reset`,
- ``reset_search_params`` restores defaults,
- override visibly changes search tree shape at fixed depth,
- unknown / out-of-range inputs raise ``ValueError``.
"""

from __future__ import annotations

import pytest

from hammerhead import Bot
from hammerhead.config import CONFIG


def _seed(bot: Bot) -> None:
    """Reproducible mid-game position."""
    bot.play((0, 0))
    bot.play((1, 0))
    bot.play((2, 0))
    bot.play((-1, 1))
    bot.play((0, 1))
    bot.play((1, -1))


def test_search_params_defaults_match_config() -> None:
    bot = Bot()
    d = bot.search_params()
    assert d["lmr_min_depth"] == CONFIG.search.lmr_min_depth
    assert d["lmr_min_move_index"] == CONFIG.search.lmr_min_move_index
    assert d["lmr_reduction"] == CONFIG.search.lmr_reduction
    # Sprint 4C — aspiration + extension knobs.
    assert d["asp_window_initial"] == CONFIG.search.asp_window_initial
    assert (
        d["asp_window_widen_factor"]
        == CONFIG.search.asp_window_widen_factor
    )
    assert (
        d["max_check_extensions"] == CONFIG.search.max_check_extensions
    )
    assert d["qsearch_max_plies"] == CONFIG.search.qsearch_max_plies


def test_set_search_params_empty_dict_is_noop() -> None:
    bot = Bot()
    before = bot.search_params()
    bot.set_search_params({})
    assert bot.search_params() == before


def test_partial_set_preserves_other_keys() -> None:
    bot = Bot()
    before = bot.search_params()
    bot.set_search_params({"lmr_reduction": 2})
    after = bot.search_params()
    assert after["lmr_reduction"] == 2
    assert after["lmr_min_depth"] == before["lmr_min_depth"]
    assert after["lmr_min_move_index"] == before["lmr_min_move_index"]


def test_override_persists_across_reset() -> None:
    bot = Bot()
    bot.set_search_params({"lmr_min_depth": 4})
    bot.reset()
    assert bot.search_params()["lmr_min_depth"] == 4


def test_reset_search_params_restores_defaults() -> None:
    bot = Bot()
    bot.set_search_params({"lmr_min_depth": 4, "lmr_reduction": 2})
    bot.reset_search_params()
    d = bot.search_params()
    assert d["lmr_min_depth"] == CONFIG.search.lmr_min_depth
    assert d["lmr_reduction"] == CONFIG.search.lmr_reduction


def test_override_changes_search_node_count() -> None:
    """At fixed depth, aggressive LMR visits fewer nodes than disabled."""
    a = Bot()
    _seed(a)
    a.set_search_params({"lmr_reduction": 3,
                         "lmr_min_move_index": 2,
                         "lmr_min_depth": 2})
    _, stats_a = a.suggest(depth=5, return_stats=True)

    b = Bot()
    _seed(b)
    b.set_search_params({"lmr_reduction": 0})
    _, stats_b = b.suggest(depth=5, return_stats=True)

    assert stats_a.nodes < stats_b.nodes, (
        f"agg={stats_a.nodes} disabled={stats_b.nodes}"
    )


def test_unknown_key_raises() -> None:
    bot = Bot()
    with pytest.raises(ValueError):
        bot.set_search_params({"not_a_key": 1})


def test_lmr_min_depth_out_of_range_raises() -> None:
    bot = Bot()
    with pytest.raises(ValueError):
        bot.set_search_params({"lmr_min_depth": 0})
    with pytest.raises(ValueError):
        bot.set_search_params({"lmr_min_depth": 33})


def test_lmr_reduction_out_of_range_raises() -> None:
    bot = Bot()
    with pytest.raises(ValueError):
        bot.set_search_params({"lmr_reduction": -1})
    with pytest.raises(ValueError):
        bot.set_search_params({"lmr_reduction": 5})


# ── Sprint 4C — aspiration + extension knobs ──────────────────────────


def test_asp_window_initial_set_and_get() -> None:
    bot = Bot()
    bot.set_search_params({"asp_window_initial": 75})
    assert bot.search_params()["asp_window_initial"] == 75


def test_asp_window_initial_out_of_range_raises() -> None:
    bot = Bot()
    with pytest.raises(ValueError):
        bot.set_search_params({"asp_window_initial": 0})
    with pytest.raises(ValueError):
        bot.set_search_params({"asp_window_initial": 10_001})


def test_asp_widen_factor_lower_bound_enforced() -> None:
    bot = Bot()
    with pytest.raises(ValueError):
        bot.set_search_params({"asp_window_widen_factor": 1})
    bot.set_search_params({"asp_window_widen_factor": 2})
    assert bot.search_params()["asp_window_widen_factor"] == 2
    with pytest.raises(ValueError):
        bot.set_search_params({"asp_window_widen_factor": 17})


def test_max_check_extensions_range() -> None:
    bot = Bot()
    bot.set_search_params({"max_check_extensions": 0})
    bot.set_search_params({"max_check_extensions": 32})
    with pytest.raises(ValueError):
        bot.set_search_params({"max_check_extensions": 33})


def test_qsearch_max_plies_range() -> None:
    bot = Bot()
    bot.set_search_params({"qsearch_max_plies": 4})
    assert bot.search_params()["qsearch_max_plies"] == 4
    with pytest.raises(ValueError):
        bot.set_search_params({"qsearch_max_plies": 33})


def test_reset_restores_aspiration_and_extensions() -> None:
    bot = Bot()
    bot.set_search_params({
        "asp_window_initial": 75,
        "asp_window_widen_factor": 4,
        "max_check_extensions": 8,
        "qsearch_max_plies": 12,
    })
    bot.reset_search_params()
    d = bot.search_params()
    assert d["asp_window_initial"] == CONFIG.search.asp_window_initial
    assert d["asp_window_widen_factor"] == CONFIG.search.asp_window_widen_factor
    assert d["max_check_extensions"] == CONFIG.search.max_check_extensions
    assert d["qsearch_max_plies"] == CONFIG.search.qsearch_max_plies
