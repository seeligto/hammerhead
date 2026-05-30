"""NNUE leaf-eval — Python-surface regression tests.

Covers:
- the net is the leaf eval on a default engine (on by default),
- the committed net artifact round-trips through the runtime override
  (proves the build.rs-codegen'd weights are byte-faithful to the JSON),
- int16-quant vs float inference agree within a small bound,
- ``clear_nnue`` reverts to the deterministic hand-built positional eval.
"""

from __future__ import annotations

import json
from pathlib import Path

from hammerhead import Bot

NET_FILE = (
    Path(__file__).resolve().parents[2]
    / "hammerhead-engine"
    / "nets"
    / "peraxis_aug.json"
)

# A non-terminal midgame (mate/fork logic must not fire — the net is live).
MIDGAME = [(0, 0), (1, 0), (0, 1), (-1, 1), (2, 0), (1, -1), (0, 2), (-1, 0)]


def _seed(bot: Bot) -> None:
    for m in MIDGAME:
        bot.play(m)


def _load_net(quantize: bool) -> dict:
    net = json.loads(NET_FILE.read_text())
    net["quantize"] = quantize
    return net


def test_net_file_present() -> None:
    """The eval ships in the repo."""
    assert NET_FILE.is_file(), f"missing committed net artifact: {NET_FILE}"


def test_nnue_on_by_default() -> None:
    """A fresh engine evaluates via the net, not the hand-built eval."""
    bot = Bot()
    _seed(bot)
    net_eval = bot.evaluate()
    bot.clear_nnue()
    hand_eval = bot.evaluate()
    assert net_eval != hand_eval, "net eval should differ from hand-built eval"


def test_committed_net_roundtrips_through_override() -> None:
    """Reloading the committed JSON via the override reproduces the default
    engine's eval exactly — the codegen'd weights match the artifact."""
    bot = Bot()
    _seed(bot)
    default_eval = bot.evaluate()  # embedded production net

    bot.set_nnue(_load_net(quantize=True))
    assert bot.evaluate() == default_eval


def test_quant_vs_float_bounded() -> None:
    """int16-quant inference tracks float inference within a small bound."""
    qbot, fbot = Bot(), Bot()
    _seed(qbot)
    _seed(fbot)
    qbot.set_nnue(_load_net(quantize=True))
    fbot.set_nnue(_load_net(quantize=False))
    # out_scale = 600; quant error is a handful of score units.
    assert abs(qbot.evaluate() - fbot.evaluate()) <= 20


def test_clear_nnue_falls_back_to_handbuilt() -> None:
    """clear_nnue reverts to the deterministic hand-built positional eval."""
    a, b = Bot(), Bot()
    _seed(a)
    _seed(b)
    a.clear_nnue()
    b.clear_nnue()
    assert a.evaluate() == b.evaluate()  # deterministic
    # And it really changed the eval vs the net path.
    c = Bot()
    _seed(c)
    assert c.evaluate() != a.evaluate()
