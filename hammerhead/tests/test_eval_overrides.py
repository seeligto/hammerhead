"""Phase 28B-1 — Python-surface tests for runtime eval overrides.

Covers:
- the ``Bot.eval_overrides`` getter returns the codegen'd defaults,
- partial-dict ``set_eval_overrides`` patches only the specified keys,
- empty-dict + default-dict are byte-identical to never calling the
  setter (the byte-identical-default gate),
- overrides persist across :meth:`Bot.reset`,
- unknown / malformed inputs raise ``ValueError``.
"""

from __future__ import annotations

import pytest

from hammerhead import Bot
from hammerhead.config import CONFIG


def _seed_position(bot: Bot) -> None:
    """A small reproducible position with Layer-1 / Layer-2 content."""
    bot.play((0, 0))  # X opens
    bot.play((1, 0))  # O
    bot.play((2, 0))
    bot.play((-1, 1))
    bot.play((3, 0))




def test_eval_overrides_defaults_match_config() -> None:
    bot = Bot()
    d = bot.eval_overrides()
    assert d["open_5"] == CONFIG.eval.open_5
    assert d["closed_5"] == CONFIG.eval.closed_5
    assert d["open_4"] == CONFIG.eval.open_4
    assert d["closed_4"] == CONFIG.eval.closed_4
    assert d["open_3"] == CONFIG.eval.open_3
    assert d["closed_3"] == CONFIG.eval.closed_3
    assert d["open_2"] == CONFIG.eval.open_2
    assert list(d["window_k_scores"]) == list(CONFIG.eval.window_k_scores)
    assert d["open_extension_factor"] == CONFIG.eval.open_extension_factor
    assert d["closed_extension_factor"] == CONFIG.eval.closed_extension_factor
    assert d["fork_cover2_bonus"] == CONFIG.eval.fork_cover2_bonus


def test_set_eval_overrides_empty_dict_is_noop() -> None:
    """Empty-dict setter must leave eval byte-identical."""
    a = Bot()
    _seed_position(a)
    baseline = a.evaluate()

    b = Bot()
    _seed_position(b)
    b.set_eval_overrides({})
    assert b.evaluate() == baseline


def test_set_eval_overrides_defaults_is_noop() -> None:
    """Setting the codegen defaults explicitly is byte-identical."""
    a = Bot()
    _seed_position(a)
    baseline = a.evaluate()

    b = Bot()
    _seed_position(b)
    b.set_eval_overrides(a.eval_overrides())
    assert b.evaluate() == baseline


def test_partial_set_preserves_other_keys() -> None:
    bot = Bot()
    before = bot.eval_overrides()
    bot.set_eval_overrides({"open_4": before["open_4"] + 999})
    after = bot.eval_overrides()
    assert after["open_4"] == before["open_4"] + 999
    # Everything else identical.
    for k in (
        "open_5",
        "closed_5",
        "closed_4",
        "open_3",
        "closed_3",
        "open_2",
        "open_extension_factor",
        "closed_extension_factor",
        "fork_cover2_bonus",
    ):
        assert after[k] == before[k]
    assert list(after["window_k_scores"]) == list(before["window_k_scores"])


def test_window_k_override_changes_eval() -> None:
    """Bumping a Layer-1 window-k score rebuilds the runtime
    `WINDOW_SCORE_8` table, so any non-trivial position with stones
    visible to the scan returns a different eval. Uses k=2 (a count
    that fires on almost any midgame fragment).

    `eval_overrides` tune the hand-built Layer-1/2/3 eval, which the NNUE
    leaf eval (on by default) bypasses — so clear the net first to exercise
    the path the override actually affects."""
    a = Bot()
    _seed_position(a)
    a.clear_nnue()
    base = a.evaluate()
    new_k = list(CONFIG.eval.window_k_scores)
    new_k[2] += 1_000
    a.set_eval_overrides({"window_k_scores": new_k})
    assert a.evaluate() != base


def test_window_k_override_accepts_list_and_tuple() -> None:
    bot = Bot()
    base = list(CONFIG.eval.window_k_scores)
    tweaked = list(base)
    tweaked[3] = base[3] + 100
    bot.set_eval_overrides({"window_k_scores": tweaked})
    assert list(bot.eval_overrides()["window_k_scores"]) == tweaked

    bot.set_eval_overrides({"window_k_scores": tuple(base)})
    assert list(bot.eval_overrides()["window_k_scores"]) == base


def test_overrides_persist_across_reset() -> None:
    bot = Bot()
    new = CONFIG.eval.open_5 + 17
    bot.set_eval_overrides({"open_5": new})
    bot.reset()
    assert bot.eval_overrides()["open_5"] == new


def test_unknown_key_raises() -> None:
    bot = Bot()
    with pytest.raises(ValueError):
        bot.set_eval_overrides({"not_a_real_key": 1})


def test_window_k_wrong_length_raises() -> None:
    bot = Bot()
    with pytest.raises(ValueError):
        bot.set_eval_overrides({"window_k_scores": [1, 2, 3]})
