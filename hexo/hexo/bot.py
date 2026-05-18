from dataclasses import dataclass

from hexo_engine import Engine

from .config import CONFIG


@dataclass
class BotConfig:
    time_per_move_ms: int = CONFIG.search.default_time_ms
    max_depth: int | None = None
    tt_size_mb: int = CONFIG.tt.default_size_mb


class Bot:
    def __init__(self, cfg: BotConfig | None = None):
        self.cfg = cfg if cfg is not None else BotConfig()
        self.engine = Engine(tt_size_mb=self.cfg.tt_size_mb)

    def play(self) -> tuple[int, int]:
        return self.engine.best_move(
            time_ms=self.cfg.time_per_move_ms,
            depth=self.cfg.max_depth,
        )

    def observe(self, move: tuple[int, int]) -> None:
        self.engine.place(move)

    def reset(self) -> None:
        self.engine.reset()
