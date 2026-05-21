"""Smoke tests for the macro benchmark library and diff tool."""

from __future__ import annotations

import argparse
import json
from pathlib import Path

import pytest

from hammerhead import benchmark as bench
from hammerhead.cli import _bench_diff
from hammerhead.config import CONFIG


def test_load_fixture_midgame_12_has_12_pieces():
    eng = bench.load_fixture("midgame_12")
    assert eng.ply() == 12


def test_load_fixture_unknown_raises():
    with pytest.raises(KeyError):
        bench.load_fixture("definitely_not_a_fixture")


def test_bench_nps_returns_positive():
    r = bench.bench_nps(fixture="single_origin", time_ms=50, runs=1)
    assert isinstance(r, bench.NpsResult)
    assert r.fixture == "single_origin"
    assert r.time_ms == 50
    assert r.nodes > 0
    assert r.nps > 0.0


def test_bench_depth_at_time_reaches_at_least_one():
    r = bench.bench_depth_at_time(fixture="empty", time_ms=50)
    assert isinstance(r, bench.DepthAtTimeResult)
    assert r.depth_reached >= 1


def test_cycles_per_node_basic():
    assert bench.cycles_per_node(100_000, 1.0, cpu_ghz=4.0) == 40_000.0


def test_cycles_per_node_zero_nodes_is_inf():
    assert bench.cycles_per_node(0, 1.0, cpu_ghz=4.0) == float("inf")


def test_detect_cpu_ghz_positive():
    assert bench.detect_cpu_ghz() > 0.0


def test_bench_quick_smoke():
    r = bench.bench_quick(fixture="empty", time_ms=10, runs=2)
    assert isinstance(r, bench.QuickResult)
    assert r.fixture == "empty"
    assert r.time_ms == 10
    assert r.runs == 2
    assert r.nps_mean > 0.0
    assert r.cycles_per_node_mean > 0.0
    assert r.depth_reached >= 1


def test_bench_quick_rejects_zero_runs():
    with pytest.raises(ValueError):
        bench.bench_quick(fixture="empty", time_ms=10, runs=0)


def test_bench_perf_smoke():
    rows = bench.bench_perf(
        fixtures=["empty", "single_origin"], time_ms_buckets=[10], runs=2
    )
    assert len(rows) == 2
    assert all(isinstance(r, bench.QuickResult) for r in rows)
    assert {r.fixture for r in rows} == {"empty", "single_origin"}
    assert all(r.nps_mean > 0.0 for r in rows)
    assert all(r.cycles_per_node_mean > 0.0 for r in rows)


def test_bench_threat_latency_positive_times():
    r = bench.bench_threat_latency(fixture="midgame_12", n_calls=10)
    assert isinstance(r, bench.ThreatLatencyResult)
    assert r.samples == 10
    assert r.cold_us > 0.0
    assert r.warm_us > 0.0


def test_bench_selfplay_completes_within_max_plies():
    r = bench.bench_selfplay(time_per_stone_ms=20, games=1, max_plies=20)
    assert isinstance(r, bench.SelfplayThroughputResult)
    assert r.games == 1
    assert 0 < r.plies_total <= 20


def test_canonical_json_roundtrip(tmp_path: Path):
    payload = {
        "schema_version": CONFIG.bench.schema_version,
        "timestamp": "2026-05-19T00:00:00Z",
        "git_sha": "abcdef0",
        "rustc_version": "rustc 1.85.0",
        "host": {"cpu": "x", "cores": 8},
        "micro": [
            {
                "group": "threats::compute_full",
                "name": "midgame_30",
                "median_ns": 4321.0,
                "mad_ns": 87.0,
                "samples": 100,
            }
        ],
        "macro": {
            "nps": [
                {
                    "fixture": "midgame_12",
                    "time_ms": 1000,
                    "depth_reached": 6,
                    "nodes": 100000,
                    "nps": 100000.0,
                }
            ],
            "depth_at_time": [],
            "threat_latency": [],
            "selfplay_throughput": [],
        },
    }
    path = tmp_path / "a.json"
    path.write_text(json.dumps(payload))
    loaded = json.loads(path.read_text())
    assert loaded == payload


def _make_args(a: Path, b: Path) -> argparse.Namespace:
    return argparse.Namespace(a=str(a), b=str(b))


def _payload(*, nps_value: float = 100000.0) -> dict:
    return {
        "schema_version": CONFIG.bench.schema_version,
        "timestamp": "2026-05-19T00:00:00Z",
        "git_sha": "abcdef0",
        "rustc_version": "rustc",
        "host": {"cpu": "x", "cores": 8},
        "micro": [
            {
                "group": "threats::compute_full",
                "name": "midgame_30",
                "median_ns": 1000.0,
                "mad_ns": 20.0,
                "samples": 100,
            }
        ],
        "macro": {
            "nps": [
                {
                    "fixture": "midgame_12",
                    "time_ms": 1000,
                    "depth_reached": 6,
                    "nodes": 100000,
                    "nps": nps_value,
                }
            ],
            "depth_at_time": [],
            "threat_latency": [],
            "selfplay_throughput": [],
        },
    }


def test_diff_identical_returns_zero(tmp_path: Path, capsys):
    a = tmp_path / "a.json"
    b = tmp_path / "b.json"
    payload = _payload()
    a.write_text(json.dumps(payload))
    b.write_text(json.dumps(payload))
    rc = _bench_diff(_make_args(a, b))
    assert rc == 0


def test_diff_regression_returns_one(tmp_path: Path, capsys):
    a = tmp_path / "a.json"
    b = tmp_path / "b.json"
    # B is 10% slower on the micro bench (median_ns higher).
    a_payload = _payload()
    b_payload = _payload()
    b_payload["micro"][0]["median_ns"] = 1100.0  # +10%
    a.write_text(json.dumps(a_payload))
    b.write_text(json.dumps(b_payload))
    rc = _bench_diff(_make_args(a, b))
    assert rc == 1


def test_bench_reference_returns_per_depth_rows():
    rows = bench.bench_reference(
        fixtures=["empty"], max_depth=3, budget_s=5.0
    )
    assert len(rows) == 3
    depths = [r.depth for r in rows]
    assert depths == [1, 2, 3]
    fixtures = {r.fixture for r in rows}
    assert fixtures == {"empty"}
    # Node counts should be non-decreasing with depth at the empty board.
    nodes = [r.nodes for r in rows]
    assert nodes == sorted(nodes)
    for r in rows:
        assert isinstance(r, bench.ReferenceEntry)
        assert r.tt_hit_rate is None


def test_bench_reference_deterministic():
    a = bench.bench_reference(
        fixtures=["single_origin"], max_depth=4, budget_s=10.0
    )
    b = bench.bench_reference(
        fixtures=["single_origin"], max_depth=4, budget_s=10.0
    )
    a_nodes = [(r.fixture, r.depth, r.nodes) for r in a]
    b_nodes = [(r.fixture, r.depth, r.nodes) for r in b]
    assert a_nodes == b_nodes


def test_bench_reference_truncates_on_tight_budget():
    # 1ms budget — at most one depth per fixture before the loop bails.
    rows = bench.bench_reference(
        fixtures=["midgame_30"], max_depth=8, budget_s=0.001
    )
    # Loop checks elapsed > budget AFTER each search, so at least the
    # first depth always runs.
    assert 1 <= len(rows) <= 8
    for r in rows:
        assert r.fixture == "midgame_30"


def test_bench_reference_rejects_invalid_args():
    with pytest.raises(ValueError):
        bench.bench_reference(fixtures=["empty"], max_depth=0, budget_s=1.0)
    with pytest.raises(ValueError):
        bench.bench_reference(fixtures=["empty"], max_depth=1, budget_s=0.0)


def test_bench_scaling_returns_cell_per_budget():
    rows = bench.bench_scaling(
        fixtures=["single_origin"], time_ms_buckets=[10, 50], runs=3
    )
    assert len(rows) == 2
    keys = [(r.fixture, r.time_ms) for r in rows]
    assert keys == [("single_origin", 10), ("single_origin", 50)]
    for r in rows:
        assert isinstance(r, bench.ScalingEntry)
        assert r.nps > 0
        assert r.ci95_lo <= r.nps <= r.ci95_hi
        # depth is non-negative; very short budgets may bottom out at 0/1
        assert r.depth >= 0
        assert r.nodes > 0


def test_bench_scaling_rejects_zero_runs():
    with pytest.raises(ValueError):
        bench.bench_scaling(
            fixtures=["empty"], time_ms_buckets=[10], runs=0
        )


_FOLDED_SAMPLE = (
    # eval leaf, search context -> eval bucket
    "main;pvs_node;eval;windows8_run 60\n"
    # threats leaf -> threats bucket
    "main;pvs_node;threats::compute;run_pieces 20\n"
    # ordering leaf -> ordering bucket
    "main;pvs_node;order_moves;would_make_six 10\n"
    # proximity module token -> board bucket
    "main;pvs_node;hammerhead_engine_core::proximity::add_proximity::"
    "{closure#0} 8\n"
    # generic helper, search context, no engine token -> search_other
    "main;quiescence_node;some_generic_helper 2\n"
    # harness stack (no search context) -> excluded from engine totals
    "main;criterion;bench_function;alloc 1000\n"
)


def test_bench_breakdown_buckets_from_folded(tmp_path: Path):
    folded = tmp_path / "flamegraph-test.folded.txt"
    folded.write_text(_FOLDED_SAMPLE)
    rows = bench.bench_breakdown(folded=folded)
    fns = {r.function for r in rows}
    assert fns == {
        "eval",
        "threats",
        "moves",
        "ordering",
        "tt",
        "board",
        "search_other",
    }
    pcts = {r.function: r.pct_cycles for r in rows}
    assert all(p >= 0.0 for p in pcts.values())
    # Engine-only renormalisation: the 1000-sample harness stack is
    # excluded; the remaining 100 engine samples sum to 100 %.
    assert sum(pcts.values()) == pytest.approx(100.0, abs=1e-6)
    assert pcts["eval"] == pytest.approx(60.0, abs=1e-6)
    assert pcts["threats"] == pytest.approx(20.0, abs=1e-6)
    assert pcts["ordering"] == pytest.approx(10.0, abs=1e-6)
    assert pcts["board"] == pytest.approx(8.0, abs=1e-6)
    assert pcts["search_other"] == pytest.approx(2.0, abs=1e-6)
    # capture identity is carried in the fixture field
    assert all(r.fixture == "flamegraph-test.folded.txt" for r in rows)


def test_bench_breakdown_missing_folded_warns(tmp_path: Path, capsys):
    missing = tmp_path / "nope.folded.txt"
    rows = bench.bench_breakdown(folded=missing)
    assert rows == []
    assert "make flamegraph" in capsys.readouterr().err


def test_diff_schema_mismatch_rejects(tmp_path: Path, capsys):
    a = tmp_path / "a.json"
    b = tmp_path / "b.json"
    a_payload = _payload()
    b_payload = _payload()
    b_payload["schema_version"] = CONFIG.bench.schema_version + 1
    a.write_text(json.dumps(a_payload))
    b.write_text(json.dumps(b_payload))
    rc = _bench_diff(_make_args(a, b))
    assert rc == 1
