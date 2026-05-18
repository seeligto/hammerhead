from __future__ import annotations

from .bot import Bot


class MatchResult:
    pass


class Standings:
    pass


class WinRate:
    pass


def match(bot_a: Bot, bot_b: Bot, max_plies: int = 200) -> MatchResult:
    raise NotImplementedError


def tournament(bots: list[Bot], rounds: int) -> Standings:
    raise NotImplementedError


def vs_sealbot(bot: Bot, num_games: int) -> WinRate:
    raise NotImplementedError


def perft(engine, depth: int) -> int:
    raise NotImplementedError
