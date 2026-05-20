"""Engine configuration loader.

Reads the single source-of-truth ``hexo.toml`` at workspace root. See
``SPEC_CONFIG.md``. Mirrors the constants codegen'd into the Rust crate at
build time so that Python-side tooling (benchmarks, analysis, CLI) sees the
same values.

Discovery order:
    1. ``$HEXO_CONFIG`` env var (absolute path), if set.
    2. Walk up from this file's location looking for ``hexo.toml``.
"""

from __future__ import annotations

import os
import sys
import tomllib
from dataclasses import dataclass
from functools import lru_cache
from pathlib import Path

if sys.version_info < (3, 11):
    raise RuntimeError("hammerhead requires Python >= 3.11 (tomllib)")


CONFIG_FILENAME = "hexo.toml"
CONFIG_ENV_VAR = "HEXO_CONFIG"


def _find_config() -> Path:
    override = os.environ.get(CONFIG_ENV_VAR)
    if override:
        p = Path(override)
        if not p.is_file():
            raise FileNotFoundError(f"{CONFIG_ENV_VAR}={override}: not a file")
        return p

    here = Path(__file__).resolve()
    for parent in here.parents:
        candidate = parent / CONFIG_FILENAME
        if candidate.is_file():
            return candidate
    raise FileNotFoundError(
        f"{CONFIG_FILENAME} not found in any parent of {here}. "
        f"Set {CONFIG_ENV_VAR} to point at it."
    )


@dataclass(frozen=True, slots=True)
class EvalConfig:
    mate_score: int
    open_5: int
    closed_5: int
    open_4: int
    closed_4: int
    open_3: int
    rhombus: int
    arch: int
    bone: int
    trapezoid: int
    open_2: int
    closed_3: int
    triangle: int
    window_k_scores: tuple[int, ...]
    open_extension_factor: int
    closed_extension_factor: int
    fork_cover2_bonus: int
    overlap_bonus_x10: int
    eval_s1s2_default: bool


@dataclass(frozen=True, slots=True)
class ThreatsConfig:
    recompute_radius: int
    cluster_radius: int
    max_s0_instances_per_player: int
    max_incremental_centers: int


@dataclass(frozen=True, slots=True)
class TTConfig:
    default_size_mb: int


@dataclass(frozen=True, slots=True)
class SearchConfigDefaults:
    default_max_depth: int
    default_time_ms: int
    default_move_radius: int
    extended_move_radius: int
    full_legality_radius: int
    move_cap: int
    deadline_check_nodes: int
    aspiration_start_depth: int
    move_gen_inner_radius: int
    move_gen_outer_radius: int
    time_stone1_pct: float
    asp_window_initial: int
    asp_window_widen_factor: int
    lmr_min_depth: int
    lmr_min_move_index: int
    lmr_reduction: int
    qsearch_max_plies: int
    max_check_extensions: int


@dataclass(frozen=True, slots=True)
class OrderingConfig:
    move_gen_cap: int
    killer_slots: int
    max_ply: int
    history_cutoff_max: int
    history_decay_num: int
    history_decay_den: int


@dataclass(frozen=True, slots=True)
class BoardConfig:
    max_piece_distance: int
    zobrist_window: int


@dataclass(frozen=True, slots=True)
class BotConfigDefaults:
    """Defaults consumed by ``hammerhead.Bot.__init__`` (Python side only)."""

    default_time_per_move_ms: int
    default_tt_size_mb: int


@dataclass(frozen=True, slots=True)
class BenchReferenceConfig:
    """Reference node-count config. See ``specs/SPEC_BENCHMARKS.md``."""

    fixtures: tuple[str, ...]
    max_depth: int
    budget_s: int


@dataclass(frozen=True, slots=True)
class BenchScalingConfig:
    """ms-time scaling config. See ``specs/SPEC_BENCHMARKS.md``."""

    fixtures: tuple[str, ...]
    time_ms: tuple[int, ...]
    runs: int


@dataclass(frozen=True, slots=True)
class BenchBreakdownConfig:
    """Per-function cycles breakdown config. See ``specs/SPEC_BENCHMARKS.md``."""

    fixtures: tuple[str, ...]
    depth: int


@dataclass(frozen=True, slots=True)
class BenchQuickConfig:
    """Inner-loop bench tier config. See ``specs/SPEC_BENCHMARKS.md``."""

    default_fixture: str
    default_time_ms: int
    default_runs: int


@dataclass(frozen=True, slots=True)
class BenchPerfConfig:
    """Pre-commit bench tier config. See ``specs/SPEC_BENCHMARKS.md``."""

    fixtures: tuple[str, ...]
    time_ms: tuple[int, ...]
    runs: int


@dataclass(frozen=True, slots=True)
class BenchVsConfig:
    """Parallel match harness config (Phase 17). See
    ``specs/SPEC_BENCHMARKS.md`` § Parallel match harness."""

    default_n_workers: int
    max_tt_mb_per_worker: int
    default_time_ms: int
    default_n_games: int


@dataclass(frozen=True, slots=True)
class BenchConfig:
    """Benchmark suite defaults. See ``specs/SPEC_BENCHMARKS.md``."""

    default_time_ms: int
    default_runs: int
    default_games: int
    default_max_plies: int
    results_dir: str
    fixtures_path: str
    schema_version: int
    reference: BenchReferenceConfig
    scaling: BenchScalingConfig
    breakdown: BenchBreakdownConfig
    quick: BenchQuickConfig
    perf: BenchPerfConfig
    vs: BenchVsConfig


@dataclass(frozen=True, slots=True)
class PromoteConfig:
    """Promotion harness defaults. See ``specs/SPEC_ROADMAP.md`` § Phase 11."""

    default_n_games: int
    default_time_ms_per_stone: int
    default_test: str
    sprt_elo_low: float
    sprt_elo_high: float
    sprt_alpha: float
    sprt_beta: float
    wilson_min_lower: float
    raw_min_winrate: float
    color_balance: bool
    opening_diversity: bool
    bestref_path: str
    worktree_path: str
    default_max_plies: int


@dataclass(frozen=True, slots=True)
class HexoConfig:
    eval: EvalConfig
    threats: ThreatsConfig
    tt: TTConfig
    search: SearchConfigDefaults
    ordering: OrderingConfig
    board: BoardConfig
    bot: BotConfigDefaults
    bench: BenchConfig
    promote: PromoteConfig
    source_path: Path


@lru_cache(maxsize=1)
def load() -> HexoConfig:
    """Load and cache the workspace ``hexo.toml``."""
    path = _find_config()
    with path.open("rb") as fh:
        raw = tomllib.load(fh)

    engine = raw["engine"]
    e = engine["eval"]
    t = engine["threats"]
    tt = engine["tt"]
    s = engine["search"]
    o = engine["ordering"]
    b = engine["board"]
    bot = raw["bot"]
    bench = raw["bench"]
    promote = raw["promote"]

    return HexoConfig(
        eval=EvalConfig(
            mate_score=e["mate_score"],
            open_5=e["open_5"],
            closed_5=e["closed_5"],
            open_4=e["open_4"],
            closed_4=e["closed_4"],
            open_3=e["open_3"],
            rhombus=e["rhombus"],
            arch=e["arch"],
            bone=e["bone"],
            trapezoid=e["trapezoid"],
            open_2=e["open_2"],
            closed_3=e["closed_3"],
            triangle=e["triangle"],
            window_k_scores=tuple(e["window_k_scores"]),
            open_extension_factor=e["open_extension_factor"],
            closed_extension_factor=e["closed_extension_factor"],
            fork_cover2_bonus=e["fork_cover2_bonus"],
            overlap_bonus_x10=e["overlap_bonus_x10"],
            eval_s1s2_default=bool(e["eval_s1s2_default"]),
        ),
        threats=ThreatsConfig(
            recompute_radius=t["recompute_radius"],
            cluster_radius=t["cluster_radius"],
            max_s0_instances_per_player=t["max_s0_instances_per_player"],
            max_incremental_centers=t["max_incremental_centers"],
        ),
        tt=TTConfig(
            default_size_mb=tt["default_size_mb"],
        ),
        search=SearchConfigDefaults(
            default_max_depth=s["default_max_depth"],
            default_time_ms=s["default_time_ms"],
            default_move_radius=s["default_move_radius"],
            extended_move_radius=s["extended_move_radius"],
            full_legality_radius=s["full_legality_radius"],
            move_cap=s["move_cap"],
            deadline_check_nodes=s["deadline_check_nodes"],
            aspiration_start_depth=s["aspiration_start_depth"],
            move_gen_inner_radius=s["move_gen_inner_radius"],
            move_gen_outer_radius=s["move_gen_outer_radius"],
            time_stone1_pct=float(s["time_stone1_pct"]),
            asp_window_initial=s["asp_window_initial"],
            asp_window_widen_factor=s["asp_window_widen_factor"],
            lmr_min_depth=s["lmr_min_depth"],
            lmr_min_move_index=s["lmr_min_move_index"],
            lmr_reduction=s["lmr_reduction"],
            qsearch_max_plies=s["qsearch_max_plies"],
            max_check_extensions=s["max_check_extensions"],
        ),
        ordering=OrderingConfig(
            move_gen_cap=o["move_gen_cap"],
            killer_slots=o["killer_slots"],
            max_ply=o["max_ply"],
            history_cutoff_max=o["history_cutoff_max"],
            history_decay_num=o["history_decay_num"],
            history_decay_den=o["history_decay_den"],
        ),
        board=BoardConfig(
            max_piece_distance=b["max_piece_distance"],
            zobrist_window=b["zobrist_window"],
        ),
        bot=BotConfigDefaults(
            default_time_per_move_ms=bot["default_time_per_move_ms"],
            default_tt_size_mb=bot["default_tt_size_mb"],
        ),
        bench=BenchConfig(
            default_time_ms=bench["default_time_ms"],
            default_runs=bench["default_runs"],
            default_games=bench["default_games"],
            default_max_plies=bench["default_max_plies"],
            results_dir=bench["results_dir"],
            fixtures_path=bench["fixtures_path"],
            schema_version=bench["schema_version"],
            reference=BenchReferenceConfig(
                fixtures=tuple(bench["reference"]["fixtures"]),
                max_depth=bench["reference"]["max_depth"],
                budget_s=bench["reference"]["budget_s"],
            ),
            scaling=BenchScalingConfig(
                fixtures=tuple(bench["scaling"]["fixtures"]),
                time_ms=tuple(bench["scaling"]["time_ms"]),
                runs=bench["scaling"]["runs"],
            ),
            breakdown=BenchBreakdownConfig(
                fixtures=tuple(bench["breakdown"]["fixtures"]),
                depth=bench["breakdown"]["depth"],
            ),
            quick=BenchQuickConfig(
                default_fixture=bench["quick"]["default_fixture"],
                default_time_ms=bench["quick"]["default_time_ms"],
                default_runs=bench["quick"]["default_runs"],
            ),
            perf=BenchPerfConfig(
                fixtures=tuple(bench["perf"]["fixtures"]),
                time_ms=tuple(bench["perf"]["time_ms"]),
                runs=bench["perf"]["runs"],
            ),
            vs=BenchVsConfig(
                default_n_workers=bench["vs"]["default_n_workers"],
                max_tt_mb_per_worker=bench["vs"]["max_tt_mb_per_worker"],
                default_time_ms=bench["vs"]["default_time_ms"],
                default_n_games=bench["vs"]["default_n_games"],
            ),
        ),
        promote=PromoteConfig(
            default_n_games=promote["default_n_games"],
            default_time_ms_per_stone=promote["default_time_ms_per_stone"],
            default_test=promote["default_test"],
            sprt_elo_low=float(promote["sprt_elo_low"]),
            sprt_elo_high=float(promote["sprt_elo_high"]),
            sprt_alpha=float(promote["sprt_alpha"]),
            sprt_beta=float(promote["sprt_beta"]),
            wilson_min_lower=float(promote["wilson_min_lower"]),
            raw_min_winrate=float(promote["raw_min_winrate"]),
            color_balance=bool(promote["color_balance"]),
            opening_diversity=bool(promote["opening_diversity"]),
            bestref_path=promote["bestref_path"],
            worktree_path=promote["worktree_path"],
            default_max_plies=promote["default_max_plies"],
        ),
        source_path=path,
    )


CONFIG = load()
"""Module-level convenience handle — ``from hammerhead.config import CONFIG``."""
