"""Bot wrapper smoke tests."""

from __future__ import annotations

from hexo.bot import Bot, BotConfig


def _short_cfg() -> BotConfig:
    return BotConfig(time_per_move_ms=200)


def test_bot_constructs() -> None:
    bot = Bot(_short_cfg())
    assert bot.engine.ply() == 0
    assert bot.to_move() == 0
    assert bot.winner() is None


def test_first_turn_is_singleton_for_x() -> None:
    bot = Bot(_short_cfg())
    moves = bot.play_turn()
    assert len(moves) == 1, "X's first turn must place exactly one stone"
    assert moves[0] == (0, 0)
    # After X singleton, O is to move at halfmove 0.
    assert bot.to_move() == 1
    assert bot.halfmove() == 0


def test_second_turn_is_two_stones() -> None:
    bx = Bot(_short_cfg())
    bo = Bot(_short_cfg())
    # X places stone 1, mirror to O's engine.
    m = bx.play_stone()
    bo.observe(m)
    assert bx.halfmove() == 0
    # Now O's turn: play_turn should yield two stones.
    moves = bo.play_turn()
    assert len(moves) == 2
    # And O should be done; halfmove flips back to 0 for X.
    assert bo.halfmove() == 0
    assert bo.to_move() == 0


def test_observe_keeps_engines_in_sync() -> None:
    bx = Bot(_short_cfg())
    bo = Bot(_short_cfg())
    m1 = bx.play_stone()
    bo.observe(m1)
    assert bx.engine.hash() == bo.engine.hash()
    assert bx.engine.ply() == bo.engine.ply()


def test_winner_none_until_terminal() -> None:
    bot = Bot(_short_cfg())
    bot.play_turn()
    assert bot.winner() is None


def test_reset_clears_engine() -> None:
    bot = Bot(_short_cfg())
    bot.play_turn()
    bot.reset()
    assert bot.engine.ply() == 0
    assert bot.winner() is None
