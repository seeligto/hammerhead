"""Phase 28B-1 — wiring tests for the sweep driver.

Pure-Python coverage of the param-name parser, grid handling, stage
cell selection, atomic JSON write, and the stopping-rule markers.
End-to-end harness invocation is exercised by the manual smoke test
documented in the B-1.2 prompt — it spawns subprocess bots and runs
real games, too heavy for the pytest gate.
"""

from __future__ import annotations

import json
from pathlib import Path

import pytest

from hammerhead import tune
from hammerhead.config import CONFIG


# ─────────────────────────────────────────────────────────────────────────────
# Param-name parsing
# ─────────────────────────────────────────────────────────────────────────────


def test_parse_param_scalar() -> None:
    assert tune._parse_param_name("open_4") == ("open_4", None)
    assert tune._parse_param_name("fork_cover2_bonus") == (
        "fork_cover2_bonus",
        None,
    )


def test_parse_param_window_k_index() -> None:
    assert tune._parse_param_name("window_k_scores[5]") == (
        "window_k_scores",
        5,
    )
    assert tune._parse_param_name("window_k_scores[0]") == (
        "window_k_scores",
        0,
    )
    assert tune._parse_param_name("window_k_scores[6]") == (
        "window_k_scores",
        6,
    )


def test_parse_param_unknown_raises() -> None:
    with pytest.raises(ValueError):
        tune._parse_param_name("not_a_param")


def test_parse_param_window_k_out_of_range_raises() -> None:
    with pytest.raises(ValueError):
        tune._parse_param_name("window_k_scores[7]")
    with pytest.raises(ValueError):
        tune._parse_param_name("window_k_scores[-1]")


# ─────────────────────────────────────────────────────────────────────────────
# Baseline lookup + overrides dict
# ─────────────────────────────────────────────────────────────────────────────


def test_baseline_value_for_scalar_matches_config() -> None:
    assert tune.baseline_value_for("open_4") == CONFIG.eval.open_4
    assert tune.baseline_value_for("fork_cover2_bonus") == (
        CONFIG.eval.fork_cover2_bonus
    )


def test_baseline_value_for_window_k_matches_config() -> None:
    assert tune.baseline_value_for("window_k_scores[5]") == (
        CONFIG.eval.window_k_scores[5]
    )


def test_build_overrides_scalar_pins_single_key() -> None:
    o = tune.build_overrides_for("open_4", 42_000)
    assert o == {"open_4": 42_000}


def test_build_overrides_window_k_carries_full_array() -> None:
    o = tune.build_overrides_for("window_k_scores[3]", 9_999)
    arr = o["window_k_scores"]
    expected = list(CONFIG.eval.window_k_scores)
    expected[3] = 9_999
    assert list(arr) == expected


# ─────────────────────────────────────────────────────────────────────────────
# Grid parsing + stage cell selection
# ─────────────────────────────────────────────────────────────────────────────


def test_parse_grid_basic() -> None:
    assert tune.parse_grid("1,2,3") == [1, 2, 3]
    assert tune.parse_grid(" 30000 , 135000 ") == [30_000, 135_000]


def test_parse_grid_rejects_empty() -> None:
    with pytest.raises(ValueError):
        tune.parse_grid("")
    with pytest.raises(ValueError):
        tune.parse_grid("1,,2")


def test_select_cells_stage_a_endpoints_only() -> None:
    grid = [30_000, 45_000, 60_000, 90_000, 135_000]
    assert tune.select_cells_for_stage("A", grid) == [30_000, 135_000]


def test_select_cells_stage_a_dedupes_endpoints() -> None:
    assert tune.select_cells_for_stage("A", [5, 5]) == [5]


def test_select_cells_stage_a_needs_two_endpoints() -> None:
    with pytest.raises(ValueError):
        tune.select_cells_for_stage("A", [5])


def test_select_cells_stage_b_full_grid() -> None:
    grid = [1, 2, 3, 4, 5]
    assert tune.select_cells_for_stage("B", grid) == grid


def test_select_cells_stage_c_single_value() -> None:
    assert tune.select_cells_for_stage("C", [42]) == [42]


def test_select_cells_stage_c_rejects_multiple() -> None:
    with pytest.raises(ValueError):
        tune.select_cells_for_stage("C", [1, 2])


# ─────────────────────────────────────────────────────────────────────────────
# Atomic JSON write
# ─────────────────────────────────────────────────────────────────────────────


def test_atomic_write_round_trip(tmp_path: Path) -> None:
    path = tmp_path / "cell.json"
    payload = {"schema_version": tune.TUNE_SCHEMA_VERSION, "elo": 42.0}
    tune._atomic_write_json(path, payload)
    assert path.is_file()
    loaded = json.loads(path.read_text())
    assert loaded == payload
    # No leftover tmp file.
    assert not (tmp_path / "cell.json.tmp").exists()


def test_cell_json_path_layout(tmp_path: Path) -> None:
    p = tune._cell_json_path(tmp_path, "open_4", "B", 60_000, "20260523T000000")
    assert p == tmp_path / "open_4" / "B" / "20260523T000000" / "60000.json"
    assert p.parent.is_dir()


def test_cell_json_path_sanitises_window_k_brackets(tmp_path: Path) -> None:
    """Bracketed param names become safe path segments (no `[` / `]`)."""
    p = tune._cell_json_path(
        tmp_path, "window_k_scores[5]", "A", 2048, "20260523T000000"
    )
    rel = p.relative_to(tmp_path)
    assert "[" not in str(rel)
    assert "]" not in str(rel)


# ─────────────────────────────────────────────────────────────────────────────
# Stopping-rule markers
# ─────────────────────────────────────────────────────────────────────────────


def _mk_cell(elo: float, ci_lo: float, ci_hi: float) -> tune.CellResult:
    return tune.CellResult(
        param="open_4",
        cell_value=42_000,
        baseline_value=60_000,
        games_played=200,
        wins=0,
        losses=0,
        draws=0,
        winrate=0.5,
        wilson_lower=0.4,
        wilson_upper=0.6,
        elo=elo,
        ci_lower_elo=ci_lo,
        ci_upper_elo=ci_hi,
        workers=10,
        time_ms_per_stone=500,
    )


def test_stage_1_zero_marker_fires_when_no_cell_promotes() -> None:
    # All cells: Wilson upper < 0 → no promote-to-Stage-2.
    cells = [_mk_cell(-30.0, -50.0, -10.0), _mk_cell(-20.0, -40.0, -5.0)]
    assert tune.stage_1_zero_marker(cells)


def test_stage_1_zero_marker_silent_when_one_cell_clears() -> None:
    cells = [
        _mk_cell(-20.0, -40.0, -5.0),
        # This one passes: CI upper > 0 AND centre >= -10.
        _mk_cell(-5.0, -15.0, +5.0),
    ]
    assert not tune.stage_1_zero_marker(cells)


def test_stage_1_zero_marker_centre_floor_blocks_negative_cells() -> None:
    """Cell with CI upper > 0 but centre < -10 must NOT clear the gate."""
    cells = [_mk_cell(-15.0, -30.0, +1.0)]
    # centre -15 < -10 → fails the centre floor even though upper > 0.
    assert tune.stage_1_zero_marker(cells)


def test_stage_2_straddle_marker_fires_when_ci_straddles_zero() -> None:
    cell = _mk_cell(+10.0, -5.0, +25.0)
    assert tune.stage_2_straddle_marker(cell)


def test_stage_2_straddle_marker_silent_when_ci_below_zero() -> None:
    cell = _mk_cell(-20.0, -40.0, -5.0)
    assert not tune.stage_2_straddle_marker(cell)


def test_stage_2_straddle_marker_silent_when_ci_above_zero() -> None:
    cell = _mk_cell(+30.0, +5.0, +55.0)
    assert not tune.stage_2_straddle_marker(cell)


# ─────────────────────────────────────────────────────────────────────────────
# CLI surface — argparse smoke
# ─────────────────────────────────────────────────────────────────────────────


def test_tune_sweep_cli_help() -> None:
    """``hammerhead bench tune-sweep --help`` parses cleanly."""
    import subprocess
    import sys

    r = subprocess.run(
        [
            sys.executable,
            "-m",
            "hammerhead.cli",
            "bench",
            "tune-sweep",
            "--help",
        ],
        capture_output=True,
        text=True,
        timeout=10,
    )
    assert r.returncode == 0, r.stderr
    for needle in ("--stage", "--param", "--grid", "--games", "--out", "--smoke"):
        assert needle in r.stdout


def test_smoke_flag_forces_smoke_subtree(tmp_path: Path) -> None:
    """``--smoke`` interposes ``tune/smoke/`` so we cannot trample the
    canonical baseline.json subtree even if --out points at it."""
    ns_attrs = {
        "stage": "A",
        "param": "open_4",
        "grid": "30000,135000",
        "games": None,
        "time_ms": 500,
        "workers": 10,
        "max_plies": 400,
        "out": str(tmp_path),
        "smoke": True,
    }

    class NS:
        pass

    ns = NS()
    for k, v in ns_attrs.items():
        setattr(ns, k, v)

    args = tune.resolve_args(ns)
    assert args.smoke is True
    assert args.games == tune.SMOKE_GAMES_PER_CELL
    assert args.out_dir == (tmp_path / "tune" / "smoke").resolve()
