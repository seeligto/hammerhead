"""Tests for the Phase 18 S1/S2 eval-weight tuning sweep (`hammerhead.tune`)."""

from __future__ import annotations

import json

import pytest

from hammerhead import tune
from hammerhead.cli import _parse_anchors, _parse_cell

# ─────────────────────────────────────────────────────────────────────────────
# Seed derivation
# ─────────────────────────────────────────────────────────────────────────────


def test_cell_seed_is_deterministic():
    a = tune.cell_seed(0x1234, "B", "open_3", 0.5, 3)
    b = tune.cell_seed(0x1234, "B", "open_3", 0.5, 3)
    assert a == b
    assert 0 <= a < 2**64


def test_cell_seed_varies_with_each_input():
    base = tune.cell_seed(0x1234, "B", "open_3", 0.5, 3)
    assert tune.cell_seed(0x9999, "B", "open_3", 0.5, 3) != base
    assert tune.cell_seed(0x1234, "C", "open_3", 0.5, 3) != base
    assert tune.cell_seed(0x1234, "B", "rhombus", 0.5, 3) != base
    assert tune.cell_seed(0x1234, "B", "open_3", 1.0, 3) != base
    assert tune.cell_seed(0x1234, "B", "open_3", 0.5, 4) != base


# ─────────────────────────────────────────────────────────────────────────────
# Cell construction
# ─────────────────────────────────────────────────────────────────────────────


def test_coordinate_descent_grid_dimensions():
    cells = tune.coordinate_descent_cells(
        ["open_3", "rhombus"],
        [0.0, 0.5, 1.0],
        {"open_3": 256, "rhombus": 512},
    )
    assert len(cells) == 6
    labels = {c.label for c in cells}
    assert labels == {
        "open_3@0",
        "open_3@0.5",
        "open_3@1",
        "rhombus@0",
        "rhombus@0.5",
        "rhombus@1",
    }


def test_coordinate_descent_weight_placement():
    cells = tune.coordinate_descent_cells(
        ["rhombus"], [0.5], {"rhombus": 512}
    )
    (cell,) = cells
    # rhombus is index 1 in SHAPE_NAMES; weight = round(0.5 * 512) = 256.
    assert cell.weights == (0, 256, 0, 0, 0, 0, 0, 0)
    assert cell.shape == "rhombus"
    assert cell.alpha == 0.5


def test_coordinate_descent_alpha_zero_is_baseline():
    cells = tune.coordinate_descent_cells(["bone"], [0.0], {"bone": 999})
    assert cells[0].weights == tune.ZERO_WEIGHTS


def test_coordinate_descent_unique_seeds_per_cell():
    cells = tune.coordinate_descent_cells(
        list(tune.SHAPE_NAMES),
        [0.0, 0.5, 1.0],
        {s: 100 for s in tune.SHAPE_NAMES},
    )
    assert len({c.seed for c in cells}) == len(cells)


def test_coordinate_descent_unknown_shape_raises():
    with pytest.raises(ValueError, match="unknown shapes"):
        tune.coordinate_descent_cells(["not_a_shape"], [0.5], {"not_a_shape": 1})


def test_coordinate_descent_missing_anchor_raises():
    with pytest.raises(ValueError, match="anchor"):
        tune.coordinate_descent_cells(["open_3"], [0.5], {})


def test_vector_cell_validates_length():
    with pytest.raises(ValueError, match="8 entries"):
        tune.vector_cell("bad", (1, 2, 3))
    cell = tune.vector_cell("ok", (1, 2, 3, 4, 5, 6, 7, 8))
    assert cell.weights == (1, 2, 3, 4, 5, 6, 7, 8)
    assert cell.shape == "combined"


# ─────────────────────────────────────────────────────────────────────────────
# Per-game configs
# ─────────────────────────────────────────────────────────────────────────────


def test_game_configs_are_deterministic_and_colour_balanced():
    cell = tune.vector_cell("c", (1,) * 8)
    cfgs_a = tune._game_configs(cell, 8, 500, tune.ZERO_WEIGHTS, 200, 4)
    cfgs_b = tune._game_configs(cell, 8, 500, tune.ZERO_WEIGHTS, 200, 4)
    assert [c.seed for c in cfgs_a] == [c.seed for c in cfgs_b]
    # Colours alternate: candidate plays X on even game indices.
    assert [c.cand_is_x for c in cfgs_a] == [True, False] * 4


# ─────────────────────────────────────────────────────────────────────────────
# Resumable persistence
# ─────────────────────────────────────────────────────────────────────────────


def test_sweep_state_roundtrip(tmp_path):
    out = tmp_path / "tune.json"
    state = tune.SweepState(meta={"stage": "B"}, cells=[{"label": "x@1"}])
    tune._write_state(out, state)
    loaded = tune._load_state(out)
    assert loaded.meta == {"stage": "B"}
    assert loaded.cells == [{"label": "x@1"}]
    assert loaded.done_labels == {"x@1"}


def test_load_state_missing_file_is_fresh(tmp_path):
    state = tune._load_state(tmp_path / "absent.json")
    assert state.cells == []
    assert state.done_labels == set()


def test_run_tune_sweep_skips_already_done_cells(tmp_path):
    out = tmp_path / "tune.json"
    cell = tune.vector_cell("done@1", (1,) * 8)
    # Pre-seed the output with this cell already recorded.
    tune._write_state(
        out,
        tune.SweepState(meta={}, cells=[{"label": "done@1", "wins": 7}]),
    )
    # No pending cells ⇒ no games are played, result is the pre-seeded set.
    results = tune.run_tune_sweep(
        [cell], games=10, time_ms=1, n_workers=1, out_path=out, progress=False
    )
    assert len(results) == 1
    assert results[0]["wins"] == 7


# ─────────────────────────────────────────────────────────────────────────────
# CLI argument parsing
# ─────────────────────────────────────────────────────────────────────────────


def test_parse_anchors():
    assert _parse_anchors("open_3=256, rhombus=512") == {
        "open_3": 256,
        "rhombus": 512,
    }
    assert _parse_anchors("") == {}


def test_parse_anchors_bad_spec_raises():
    with pytest.raises(ValueError, match="anchor spec"):
        _parse_anchors("open_3")


def test_parse_cell():
    label, weights = _parse_cell("cand=0,1,2,3,4,5,6,7")
    assert label == "cand"
    assert weights == (0, 1, 2, 3, 4, 5, 6, 7)


def test_parse_cell_bad_spec_raises():
    with pytest.raises(ValueError, match="cell spec"):
        _parse_cell("0,1,2,3,4,5,6,7")


# ─────────────────────────────────────────────────────────────────────────────
# End-to-end — a tiny real sweep (plays games; kept fast)
# ─────────────────────────────────────────────────────────────────────────────


def test_run_cell_is_reproducible():
    """Same cell + seed ⇒ identical (W, L, D). The reproducibility
    guarantee the sweep relies on (STEP 3 item 3)."""
    cell = tune.vector_cell("repro", (0,) * 8, seed_base=0xABCD)
    kw = dict(games=4, time_ms=2, n_workers=2, max_plies=24)
    r1 = tune.run_cell(cell, **kw)
    r2 = tune.run_cell(cell, **kw)
    assert (r1.wins, r1.losses, r1.draws) == (r2.wins, r2.losses, r2.draws)
    assert r1.wins + r1.losses + r1.draws == 4


def test_run_tune_sweep_writes_expected_json(tmp_path):
    out = tmp_path / "tune.json"
    cell = tune.vector_cell("smoke", (0,) * 8)
    tune.run_tune_sweep(
        [cell],
        games=2,
        time_ms=2,
        n_workers=1,
        out_path=out,
        stage="B",
        max_plies=24,
        progress=False,
    )
    data = json.loads(out.read_text())
    assert data["meta"]["stage"] == "B"
    assert len(data["cells"]) == 1
    c = data["cells"][0]
    for key in (
        "shape",
        "alpha",
        "wins",
        "losses",
        "draws",
        "wilson_lb",
        "wilson_ub",
        "seed",
    ):
        assert key in c
    assert c["wins"] + c["losses"] + c["draws"] == 2
