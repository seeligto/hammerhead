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
    raise RuntimeError("hexo requires Python >= 3.11 (tomllib)")


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
    tempo_weight: int
    overlap_bonus_x10: int


@dataclass(frozen=True, slots=True)
class ThreatsConfig:
    recompute_radius: int
    cluster_radius: int
    max_s0_instances_per_player: int


@dataclass(frozen=True, slots=True)
class SearchConfigDefaults:
    default_max_depth: int
    default_time_ms: int
    default_tt_size_mb: int
    default_move_radius: int
    extended_move_radius: int
    full_legality_radius: int
    move_cap: int
    deadline_check_nodes: int
    aspiration_start_depth: int
    move_gen_inner_radius: int
    move_gen_outer_radius: int
    move_gen_cap: int


@dataclass(frozen=True, slots=True)
class BoardConfig:
    max_piece_distance: int
    zobrist_window: int


@dataclass(frozen=True, slots=True)
class HexoConfig:
    eval: EvalConfig
    threats: ThreatsConfig
    search: SearchConfigDefaults
    board: BoardConfig
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
    s = engine["search"]
    b = engine["board"]

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
            tempo_weight=e["tempo_weight"],
            overlap_bonus_x10=e["overlap_bonus_x10"],
        ),
        threats=ThreatsConfig(
            recompute_radius=t["recompute_radius"],
            cluster_radius=t["cluster_radius"],
            max_s0_instances_per_player=t["max_s0_instances_per_player"],
        ),
        search=SearchConfigDefaults(
            default_max_depth=s["default_max_depth"],
            default_time_ms=s["default_time_ms"],
            default_tt_size_mb=s["default_tt_size_mb"],
            default_move_radius=s["default_move_radius"],
            extended_move_radius=s["extended_move_radius"],
            full_legality_radius=s["full_legality_radius"],
            move_cap=s["move_cap"],
            deadline_check_nodes=s["deadline_check_nodes"],
            aspiration_start_depth=s["aspiration_start_depth"],
            move_gen_inner_radius=s["move_gen_inner_radius"],
            move_gen_outer_radius=s["move_gen_outer_radius"],
            move_gen_cap=s["move_gen_cap"],
        ),
        board=BoardConfig(
            max_piece_distance=b["max_piece_distance"],
            zobrist_window=b["zobrist_window"],
        ),
        source_path=path,
    )


CONFIG = load()
"""Module-level convenience handle — ``from hexo.config import CONFIG``."""
