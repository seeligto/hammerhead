"""Phase 11 promotion harness.

Run a match between two engine binaries (each spoken to via the Phase 9
``hammerhead bot`` subprocess protocol) and decide whether the candidate is
strong enough to promote.

See ``specs/SPEC_ROADMAP.md`` § Phase 11 and ``specs/SPEC_API.md`` for
the protocol contract.

Public surface
--------------
- ``SubprocessBot`` — line-protocol wrapper around one ``hammerhead bot`` child.
- ``run_match`` — drive an N-game match; returns ``MatchResult``.
- ``wilson_interval``, ``winrate_to_elo``, ``sprt_llr`` — pure-function
  statistics (covered by unit tests).
"""

from __future__ import annotations

import multiprocessing
import subprocess
import time
from dataclasses import dataclass
from typing import Optional

from .config import CONFIG, PromoteConfig
from .promote_sprt import (  # re-export
    elo_to_winrate,
    sprt_llr,
    sprt_thresholds,
    wilson_interval,
    winrate_to_elo,
)
from .promote_worktree import (  # re-export
    max_tt_mb_per_worker,
    resolve_worker_count,
    with_tt_bound,
)


Coord = tuple[int, int]


# ─────────────────────────────────────────────────────────────────────────────
# Subprocess protocol wrapper
# ─────────────────────────────────────────────────────────────────────────────


class BotProtocolError(RuntimeError):
    """Raised on malformed subprocess responses or unexpected EOF."""


class SubprocessBot:
    """Manages one ``hammerhead bot`` child via stdin/stdout lines.

    Use as a context manager so the child is reaped even on exceptions.
    """

    def __init__(self, cmd: list[str]) -> None:
        self.cmd = cmd
        self.proc = subprocess.Popen(
            cmd,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            bufsize=1,
        )
        try:
            banner = self._readline()
        except BotProtocolError:
            self.close()
            raise
        # The promotion harness compares the current engine against a
        # possibly-older `.bestref` worktree, which may predate the
        # hexo→hammerhead rename and emit `hexo bot ready`. Accept any
        # `<name> bot ready` banner rather than a single literal.
        if not banner.endswith("bot ready"):
            self.close()
            raise BotProtocolError(
                f"unexpected banner from {cmd!r}: {banner!r}"
            )

    # — low-level —

    def _readline(self) -> str:
        assert self.proc.stdout is not None
        line = self.proc.stdout.readline()
        if not line:
            stderr_tail = ""
            if self.proc.stderr is not None:
                try:
                    stderr_tail = self.proc.stderr.read() or ""
                except Exception:  # noqa: BLE001
                    stderr_tail = ""
            raise BotProtocolError(
                f"bot {self.cmd!r} closed stdout unexpectedly; stderr={stderr_tail!r}"
            )
        return line.rstrip("\n")

    def _send(self, line: str) -> str:
        assert self.proc.stdin is not None
        self.proc.stdin.write(line + "\n")
        self.proc.stdin.flush()
        return self._readline()

    @staticmethod
    def _expect_ok(resp: str, cmd: str) -> None:
        if resp != "ok":
            raise BotProtocolError(f"{cmd}: expected 'ok', got {resp!r}")

    # — protocol —

    def reset(self) -> None:
        self._expect_ok(self._send("reset"), "reset")

    def place(self, q: int, r: int) -> None:
        self._expect_ok(self._send(f"place {q} {r}"), f"place {q} {r}")

    def best_move(self, time_ms: int) -> Coord:
        resp = self._send(f"best_move {time_ms}")
        try:
            q_str, r_str = resp.split()
            return int(q_str), int(r_str)
        except ValueError as e:
            raise BotProtocolError(f"best_move: bad response {resp!r}") from e

    def winner(self) -> str:
        return self._send("winner")  # "X" / "O" / "none"

    def halfmove(self) -> int:
        return int(self._send("halfmove"))

    def to_move(self) -> str:
        return self._send("to_move")  # "X" / "O"

    def ply(self) -> int:
        return int(self._send("ply"))

    def quit(self) -> None:
        if self.proc.poll() is not None:
            return
        try:
            assert self.proc.stdin is not None
            self.proc.stdin.write("quit\n")
            self.proc.stdin.flush()
        except (BrokenPipeError, OSError):
            return
        # Drain "bye" so the child's stdout buffer doesn't block its exit.
        try:
            self._readline()
        except BotProtocolError:
            pass

    # — lifecycle —

    def __enter__(self) -> "SubprocessBot":
        return self

    def __exit__(self, *_a: object) -> None:
        self.close()

    def close(self) -> None:
        self.quit()
        try:
            self.proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self.proc.kill()
            self.proc.wait(timeout=2)
        finally:
            for stream in (self.proc.stdin, self.proc.stdout, self.proc.stderr):
                if stream is not None:
                    try:
                        stream.close()
                    except Exception:  # noqa: BLE001
                        pass


# ─────────────────────────────────────────────────────────────────────────────
# Match driver
# ─────────────────────────────────────────────────────────────────────────────


@dataclass(frozen=True, slots=True)
class MatchConfig:
    """Inputs to ``run_match``. Defaults come from ``CONFIG.promote``."""

    n_games: int
    time_ms_per_stone: int
    test: str  # "sprt" | "wilson" | "raw"
    sprt_elo_low: float
    sprt_elo_high: float
    sprt_alpha: float
    sprt_beta: float
    wilson_min_lower: float
    raw_min_winrate: float
    color_balance: bool
    opening_diversity: bool
    max_plies: int

    @classmethod
    def from_promote_config(
        cls,
        pc: PromoteConfig = CONFIG.promote,
        *,
        n_games: Optional[int] = None,
        time_ms_per_stone: Optional[int] = None,
        test: Optional[str] = None,
    ) -> "MatchConfig":
        return cls(
            n_games=n_games if n_games is not None else pc.default_n_games,
            time_ms_per_stone=(
                time_ms_per_stone
                if time_ms_per_stone is not None
                else pc.default_time_ms_per_stone
            ),
            test=test if test is not None else pc.default_test,
            sprt_elo_low=pc.sprt_elo_low,
            sprt_elo_high=pc.sprt_elo_high,
            sprt_alpha=pc.sprt_alpha,
            sprt_beta=pc.sprt_beta,
            wilson_min_lower=pc.wilson_min_lower,
            raw_min_winrate=pc.raw_min_winrate,
            color_balance=pc.color_balance,
            opening_diversity=pc.opening_diversity,
            max_plies=pc.default_max_plies,
        )


@dataclass(frozen=True, slots=True)
class GameResult:
    """One game's outcome from ``current`` (a)'s perspective."""

    winner: Optional[str]  # "current" | "best" | None for draw
    plies: int
    current_was_x: bool


@dataclass(frozen=True, slots=True)
class MatchResult:
    games_played: int
    current_wins: int
    best_wins: int
    draws: int
    winrate: float
    wilson_lower: float
    wilson_upper: float
    sprt_llr: Optional[float]
    sprt_verdict: str  # "accept_h1" | "accept_h0" | "continuing"
    estimated_elo: float
    estimated_elo_ci: tuple[float, float]
    final_verdict: str  # "PROMOTE" | "REJECT" | "INCONCLUSIVE"


def play_one_game(
    a: SubprocessBot,
    b: SubprocessBot,
    *,
    a_is_x: bool,
    time_ms: int,
    max_plies: int,
) -> GameResult:
    """Drive both bots through one game. Returns the outcome from a's POV.

    Both bots are reset at the start. Bot ``a`` is the source of truth
    for engine state (``to_move``/``winner``); every placement is
    mirrored to ``b`` so both engines stay in sync.
    """
    a.reset()
    b.reset()

    plies = 0
    last_winner = "none"
    while plies < max_plies:
        side = a.to_move()  # "X" or "O"
        mover_is_a = (side == "X") == a_is_x
        mover = a if mover_is_a else b
        other = b if mover_is_a else a

        q, r = mover.best_move(time_ms)
        mover.place(q, r)
        other.place(q, r)
        plies += 1

        last_winner = a.winner()
        # Cheap parity check: if engines disagree on terminal state, a
        # protocol-level desync is silently polluting results — bail loudly.
        b_winner = b.winner()
        if b_winner != last_winner:
            raise BotProtocolError(
                f"engines disagree on winner: a={last_winner!r} b={b_winner!r} "
                f"after ply {plies}"
            )
        if last_winner != "none":
            break

    if last_winner == "none":
        return GameResult(winner=None, plies=plies, current_was_x=a_is_x)

    # last_winner is "X" or "O" — translate to a/b POV.
    x_is_a = a_is_x
    if last_winner == "X":
        current_won = x_is_a
    else:
        current_won = not x_is_a
    return GameResult(
        winner="current" if current_won else "best",
        plies=plies,
        current_was_x=a_is_x,
    )


def _summarize(
    results: list[GameResult],
    cfg: MatchConfig,
    sprt_verdict: str,
    llr: Optional[float],
) -> MatchResult:
    n = len(results)
    wins = sum(1 for r in results if r.winner == "current")
    losses = sum(1 for r in results if r.winner == "best")
    draws = sum(1 for r in results if r.winner is None)
    score = wins + 0.5 * draws
    winrate = (score / n) if n else 0.0
    wl, wu = wilson_interval(score, n)
    elo_point = winrate_to_elo(winrate)
    elo_ci = (winrate_to_elo(wl), winrate_to_elo(wu))

    if cfg.test == "sprt":
        if sprt_verdict == "accept_h1":
            final = "PROMOTE"
        elif sprt_verdict == "accept_h0":
            final = "REJECT"
        else:
            final = "INCONCLUSIVE"
    elif cfg.test == "wilson":
        final = "PROMOTE" if wl >= cfg.wilson_min_lower else "REJECT"
    elif cfg.test == "raw":
        final = "PROMOTE" if winrate >= cfg.raw_min_winrate else "REJECT"
    else:
        raise ValueError(f"unknown test {cfg.test!r}")

    return MatchResult(
        games_played=n,
        current_wins=wins,
        best_wins=losses,
        draws=draws,
        winrate=winrate,
        wilson_lower=wl,
        wilson_upper=wu,
        sprt_llr=llr,
        sprt_verdict=sprt_verdict,
        estimated_elo=elo_point,
        estimated_elo_ci=elo_ci,
        final_verdict=final,
    )


def run_match(
    current_cmd: list[str],
    best_cmd: list[str],
    cfg: MatchConfig,
    *,
    on_game: Optional[callable] = None,  # type: ignore[type-arg]
) -> MatchResult:
    """Play up to ``cfg.n_games`` games; return aggregated result.

    With ``cfg.test == "sprt"`` we break early on acceptance of either
    hypothesis. ``on_game(i, result, llr)`` is called after every game
    if provided (used by the CLI for progress output).
    """
    if cfg.opening_diversity:
        raise NotImplementedError(
            "opening_diversity is reserved for follow-up; "
            "disable [promote].opening_diversity for v1"
        )

    results: list[GameResult] = []
    log_low, log_high = sprt_thresholds(cfg)
    llr: Optional[float] = None
    verdict = "continuing"

    for i in range(cfg.n_games):
        a_is_x = (i % 2 == 0) if cfg.color_balance else True
        with SubprocessBot(current_cmd) as a, SubprocessBot(best_cmd) as b:
            r = play_one_game(
                a,
                b,
                a_is_x=a_is_x,
                time_ms=cfg.time_ms_per_stone,
                max_plies=cfg.max_plies,
            )
        results.append(r)

        if cfg.test == "sprt":
            wins = sum(1 for x in results if x.winner == "current")
            losses = sum(1 for x in results if x.winner == "best")
            draws = sum(1 for x in results if x.winner is None)
            llr = sprt_llr(
                wins,
                draws,
                losses,
                elo_low=cfg.sprt_elo_low,
                elo_high=cfg.sprt_elo_high,
            )

        if on_game is not None:
            on_game(i, r, llr)

        if cfg.test == "sprt" and llr is not None:
            if llr >= log_high:
                verdict = "accept_h1"
                break
            if llr <= log_low:
                verdict = "accept_h0"
                break

    return _summarize(results, cfg, verdict, llr)


# ─────────────────────────────────────────────────────────────────────────────
# Parallel match harness (Phase 17)
# ─────────────────────────────────────────────────────────────────────────────


@dataclass(frozen=True, slots=True)
class GameConfig:
    """One game's deterministic assignment within a parallel match.

    The (game_idx → colour) mapping is fixed by ``build_game_configs``,
    so a match at a given ``n_games`` is reproducible across runs and
    worker counts (modulo timer noise — see SPEC_BENCHMARKS).
    """

    game_idx: int
    current_is_x: bool
    time_ms: int
    max_plies: int


@dataclass(frozen=True, slots=True)
class ParallelGameResult:
    """Worker → coordinator record for one completed game.

    Field-compatible with :class:`GameResult` for ``_summarize`` and
    ``on_game`` callbacks, plus ``game_idx`` / ``wall_seconds`` / ``notes``.
    """

    game_idx: int
    winner: Optional[str]  # "current" | "best" | None for draw
    plies: int
    current_was_x: bool
    wall_seconds: float
    notes: str  # "" on success; error text on crash / timeout


def build_game_configs(cfg: MatchConfig) -> list[GameConfig]:
    """Deterministic ``game_idx → GameConfig`` assignment for a match.

    Colour assignment mirrors the sequential :func:`run_match`: with
    ``color_balance``, even game indices play ``current`` as X.
    """
    return [
        GameConfig(
            game_idx=i,
            current_is_x=(i % 2 == 0) if cfg.color_balance else True,
            time_ms=cfg.time_ms_per_stone,
            max_plies=cfg.max_plies,
        )
        for i in range(cfg.n_games)
    ]


# Worker-process globals, populated by `_worker_init` (one call per worker).
_WORKER_CURRENT_CMD: list[str] = []
_WORKER_BEST_CMD: list[str] = []


def _worker_init(current_cmd: list[str], best_cmd: list[str]) -> None:
    """Pool initializer — broadcast the two engine commands once per worker."""
    global _WORKER_CURRENT_CMD, _WORKER_BEST_CMD
    _WORKER_CURRENT_CMD = current_cmd
    _WORKER_BEST_CMD = best_cmd


def _play_one_game_in_worker(gc: GameConfig) -> ParallelGameResult:
    """Worker entry point. Spawns two fresh engine subprocesses, plays one
    game, returns the result.

    Fresh engines per game is the simple correctness model: subprocess
    startup (~10-100 ms) is < 0.1 % of a 1 s/stone game. A crash is
    captured in ``notes`` rather than killing the pool — the coordinator
    excludes noted games from the tally.
    """
    start = time.monotonic()
    try:
        with SubprocessBot(_WORKER_CURRENT_CMD) as a, SubprocessBot(_WORKER_BEST_CMD) as b:
            r = play_one_game(
                a,
                b,
                a_is_x=gc.current_is_x,
                time_ms=gc.time_ms,
                max_plies=gc.max_plies,
            )
        return ParallelGameResult(
            game_idx=gc.game_idx,
            winner=r.winner,
            plies=r.plies,
            current_was_x=r.current_was_x,
            wall_seconds=time.monotonic() - start,
            notes="",
        )
    except Exception as exc:  # noqa: BLE001
        return ParallelGameResult(
            game_idx=gc.game_idx,
            winner=None,
            plies=0,
            current_was_x=gc.current_is_x,
            wall_seconds=time.monotonic() - start,
            notes=f"{type(exc).__name__}: {exc}",
        )


def _tally(results: list[ParallelGameResult]) -> tuple[int, int, int]:
    """`(wins, draws, losses)` from ``current``'s POV over OK games."""
    wins = sum(1 for r in results if r.winner == "current")
    losses = sum(1 for r in results if r.winner == "best")
    draws = sum(1 for r in results if r.winner is None)
    return wins, draws, losses


def run_match_parallel(
    current_cmd: list[str],
    best_cmd: list[str],
    cfg: MatchConfig,
    *,
    n_workers: int = 0,
    progress_every: int = 10,
) -> MatchResult:
    """Play ``cfg.n_games`` games across a process pool; aggregate.

    Games run concurrently in worker processes (``n_workers`` of them,
    0 = auto). Each game still keeps its two engines in-process to the
    worker via the subprocess-Bot model. Results are sorted by
    ``game_idx`` before summary so the aggregate is order-independent.

    SPRT mode: the coordinator recomputes the running LLR after every
    completed game and, on a decisive crossing, leaves the pool's
    ``with`` block — which terminates any games still in flight. The
    partial tail (games that had started but not finished) is simply
    discarded; the verdict stands on the games that completed.
    """
    if cfg.opening_diversity:
        raise NotImplementedError(
            "opening_diversity is reserved for follow-up; "
            "disable [promote].opening_diversity for v1"
        )
    if cfg.n_games < 1:
        raise ValueError("n_games must be >= 1")

    n_workers = resolve_worker_count(n_workers, cfg.n_games)
    configs = build_game_configs(cfg)
    log_low, log_high = sprt_thresholds(cfg)
    results: list[ParallelGameResult] = []
    llr: Optional[float] = None
    verdict = "continuing"

    ctx = multiprocessing.get_context("spawn")
    with ctx.Pool(
        processes=n_workers,
        initializer=_worker_init,
        initargs=(current_cmd, best_cmd),
    ) as pool:
        for r in pool.imap_unordered(_play_one_game_in_worker, configs):
            results.append(r)
            if r.notes:
                print(f"game {r.game_idx + 1}: FAILED — {r.notes}", flush=True)

            ok = [x for x in results if not x.notes]
            wins, draws, losses = _tally(ok)
            if cfg.test == "sprt" and ok:
                llr = sprt_llr(
                    wins,
                    draws,
                    losses,
                    elo_low=cfg.sprt_elo_low,
                    elo_high=cfg.sprt_elo_high,
                )

            if len(results) % progress_every == 0 or len(results) == cfg.n_games:
                llr_s = f"  llr={llr:+.3f}" if llr is not None else ""
                print(
                    f"progress: {len(results)}/{cfg.n_games} games  "
                    f"current {wins}-{losses}-{draws} (W-L-D){llr_s}",
                    flush=True,
                )

            if cfg.test == "sprt" and llr is not None:
                if llr >= log_high:
                    verdict = "accept_h1"
                    break
                if llr <= log_low:
                    verdict = "accept_h0"
                    break

    ok = sorted(
        (r for r in results if not r.notes), key=lambda r: r.game_idx
    )
    failed = [r for r in results if r.notes]
    if failed:
        print(
            f"warning: {len(failed)} game(s) failed; excluded from the tally",
            flush=True,
        )
    game_results = [
        GameResult(winner=r.winner, plies=r.plies, current_was_x=r.current_was_x)
        for r in ok
    ]
    return _summarize(game_results, cfg, verdict, llr)
