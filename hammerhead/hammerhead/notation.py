from __future__ import annotations


def parse_bsn(s: str) -> list[tuple[int, int]]:
    raise NotImplementedError


def dump_bsn(moves: list[tuple[int, int]]) -> str:
    raise NotImplementedError


def parse_bke(s: str) -> list[tuple[int, int]]:
    raise NotImplementedError


def dump_bke(moves: list[tuple[int, int]]) -> str:
    raise NotImplementedError


def parse_hxn(data: bytes) -> "GameRecord":
    raise NotImplementedError


def dump_hxn(record: "GameRecord") -> bytes:
    raise NotImplementedError


class GameRecord:
    pass
