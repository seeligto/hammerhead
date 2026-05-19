"""Smoke tests for the macro benchmark library and diff tool."""

from __future__ import annotations

import argparse
import json
from pathlib import Path

import pytest

from hexo import benchmark as bench
from hexo.cli import _bench_diff
from hexo.config import CONFIG


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
