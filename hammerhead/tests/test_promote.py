"""Tests for the Phase 11 promotion harness.

Covers the pure-stats core (wilson, elo, SPRT), the subprocess match
driver end-to-end at a tight time budget, color-balance accounting,
and idempotency of ``scripts/setup_worktree.sh``.
"""

from __future__ import annotations

import math
import os
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

from hammerhead import promote


_REPO_ROOT = Path(__file__).resolve().parents[2]
_SETUP_SCRIPT = _REPO_ROOT / "scripts" / "setup_worktree.sh"


# ─────────────────────────────────────────────────────────────────────────────
# Pure statistics
# ─────────────────────────────────────────────────────────────────────────────


def test_wilson_zero_n_is_full_interval() -> None:
    assert promote.wilson_interval(0, 0) == (0.0, 1.0)


def test_wilson_50_of_100_matches_reference() -> None:
    lo, hi = promote.wilson_interval(50, 100, z=1.96)
    # Standard Wilson reference value for p̂=0.5, n=100, z=1.96.
    assert lo == pytest.approx(0.4038, abs=1e-4)
    assert hi == pytest.approx(0.5962, abs=1e-4)


def test_winrate_to_elo_at_half_is_zero() -> None:
    assert promote.winrate_to_elo(0.5) == pytest.approx(0.0, abs=1e-9)


def test_winrate_to_elo_at_0_76_is_about_200() -> None:
    e = promote.winrate_to_elo(0.76)
    assert e == pytest.approx(200.0, abs=1.0)


def test_winrate_to_elo_saturates() -> None:
    assert math.isinf(promote.winrate_to_elo(0.0))
    assert math.isinf(promote.winrate_to_elo(1.0))


# ─────────────────────────────────────────────────────────────────────────────
# SPRT
# ─────────────────────────────────────────────────────────────────────────────


def _sprt_thresholds(alpha: float = 0.05, beta: float = 0.05) -> tuple[float, float]:
    log_high = math.log((1.0 - beta) / alpha)
    log_low = math.log(beta / (1.0 - alpha))
    return log_low, log_high


def test_sprt_all_wins_accepts_h1_quickly() -> None:
    """All-wins crosses log_high in a small number of games."""
    log_low, log_high = _sprt_thresholds()
    # Use a generous band so the test runs fast and is robust.
    for n in range(1, 50):
        llr = promote.sprt_llr(n, 0, 0, elo_low=0.0, elo_high=100.0)
        if llr >= log_high:
            assert n <= 20, f"all-wins should accept_h1 well within 20 games, took {n}"
            return
    pytest.fail("all-wins never crossed log_high")


def test_sprt_all_losses_accepts_h0_quickly() -> None:
    log_low, _ = _sprt_thresholds()
    for n in range(1, 50):
        llr = promote.sprt_llr(0, 0, n, elo_low=0.0, elo_high=100.0)
        if llr <= log_low:
            assert n <= 20, f"all-losses should accept_h0 well within 20 games, took {n}"
            return
    pytest.fail("all-losses never crossed log_low")


def test_sprt_balanced_stays_between_bounds() -> None:
    """Equal wins/losses (and many draws) should not trigger either bound."""
    log_low, log_high = _sprt_thresholds()
    for w, d, l in [(10, 0, 10), (5, 10, 5), (3, 4, 3)]:
        llr = promote.sprt_llr(w, d, l, elo_low=0.0, elo_high=100.0)
        assert log_low < llr < log_high


# ─────────────────────────────────────────────────────────────────────────────
# End-to-end smoke (slow): identical bots, winrate ≈ 0.5
# ─────────────────────────────────────────────────────────────────────────────


def _bot_cmd() -> list[str]:
    # Both sides spawn the same hammerhead bot. tt-size-mb=4 keeps startup quick.
    return [sys.executable, "-m", "hammerhead.cli", "bot", "--tt-size-mb", "4"]


def test_subprocess_bot_smoke_protocol() -> None:
    """``SubprocessBot`` can drive a single round-trip without errors."""
    cmd = _bot_cmd()
    with promote.SubprocessBot(cmd) as bot:
        bot.reset()
        assert bot.to_move() == "X"
        assert bot.halfmove() == 0
        bot.place(0, 0)
        assert bot.ply() == 1
        assert bot.winner() == "none"


def test_run_match_smoke_identical_bots() -> None:
    """5 games at 50ms between identical engines: harness end-to-end."""
    cfg = promote.MatchConfig.from_promote_config(
        n_games=5,
        time_ms_per_stone=50,
        test="raw",
    )
    # Smoke uses tighter caps to keep the test fast.
    cfg = promote.MatchConfig(
        n_games=cfg.n_games,
        time_ms_per_stone=cfg.time_ms_per_stone,
        test=cfg.test,
        sprt_elo_low=cfg.sprt_elo_low,
        sprt_elo_high=cfg.sprt_elo_high,
        sprt_alpha=cfg.sprt_alpha,
        sprt_beta=cfg.sprt_beta,
        wilson_min_lower=cfg.wilson_min_lower,
        raw_min_winrate=cfg.raw_min_winrate,
        color_balance=True,
        opening_diversity=False,
        max_plies=60,
    )
    res = promote.run_match(_bot_cmd(), _bot_cmd(), cfg)
    # Structural check only. Identical bots split ~50/50 in expectation,
    # but a time-limited search makes any single 5-game sample noisy —
    # a strict winrate band is flaky by nature (Phase 17 STEP 1.5).
    assert res.games_played == 5
    assert res.current_wins + res.best_wins + res.draws == 5


def test_color_balance_exact_split() -> None:
    """4 games + color_balance → exactly 2 a=X and 2 a=O."""
    cfg = promote.MatchConfig(
        n_games=4,
        time_ms_per_stone=50,
        test="raw",
        sprt_elo_low=0.0,
        sprt_elo_high=5.0,
        sprt_alpha=0.05,
        sprt_beta=0.05,
        wilson_min_lower=0.5,
        raw_min_winrate=0.6,
        color_balance=True,
        opening_diversity=False,
        max_plies=60,
    )
    observed: list[bool] = []

    def on_game(_i: int, r: promote.GameResult, _llr: object) -> None:
        observed.append(r.current_was_x)

    promote.run_match(_bot_cmd(), _bot_cmd(), cfg, on_game=on_game)
    assert sum(observed) == 2, f"expected 2 games as X, got {sum(observed)}"
    assert sum(not x for x in observed) == 2


# ─────────────────────────────────────────────────────────────────────────────
# Parallel match harness (Phase 17)
# ─────────────────────────────────────────────────────────────────────────────


def _small_match_config(n_games: int, *, test: str = "raw") -> promote.MatchConfig:
    """Tight-budget MatchConfig for fast harness tests."""
    return promote.MatchConfig(
        n_games=n_games,
        time_ms_per_stone=40,
        test=test,
        sprt_elo_low=0.0,
        sprt_elo_high=5.0,
        sprt_alpha=0.05,
        sprt_beta=0.05,
        wilson_min_lower=0.5,
        raw_min_winrate=0.6,
        color_balance=True,
        opening_diversity=False,
        max_plies=60,
    )


def test_parallel_match_determinism() -> None:
    """The (game_idx → colour) assignment is a pure deterministic
    function of the config: identical across calls, exact colour split
    under ``color_balance``.

    Game *outcomes* are not asserted — a time-limited search depends on
    wall-clock, so outcome-level determinism is out of reach. The
    deterministic contract is the config assignment, which is what
    makes a match reproducible "modulo timer noise"."""
    cfg = _small_match_config(10)
    first = promote.build_game_configs(cfg)
    second = promote.build_game_configs(cfg)
    assert first == second
    assert [g.game_idx for g in first] == list(range(10))
    assert sum(g.current_is_x for g in first) == 5


def test_parallel_match_smoke_identical_bots() -> None:
    """10 games between identical engines across a process pool.

    Flaky by nature — identical bots split ~50/50 only in expectation.
    The assertion is therefore structural: every game completed clean
    and the tally is internally consistent."""
    cfg = _small_match_config(10)
    res = promote.run_match_parallel(_bot_cmd(), _bot_cmd(), cfg, n_workers=4)
    assert res.games_played == 10
    assert res.current_wins + res.best_wins + res.draws == 10


def test_parallel_match_worker_count_1_matches_sequential() -> None:
    """N=1 worker reproduces the sequential harness's structure.

    A byte-for-byte outcome match is impossible — both harnesses run a
    time-limited (hence wall-clock-dependent) search. What must hold:
    same game count and a well-formed, internally consistent aggregate."""
    cfg = _small_match_config(4)
    seq = promote.run_match(_bot_cmd(), _bot_cmd(), cfg)
    par = promote.run_match_parallel(_bot_cmd(), _bot_cmd(), cfg, n_workers=1)
    assert seq.games_played == par.games_played == 4
    assert par.current_wins + par.best_wins + par.draws == 4


# ─────────────────────────────────────────────────────────────────────────────
# Worktree script idempotency
# ─────────────────────────────────────────────────────────────────────────────


def test_match_config_override_precedence() -> None:
    """``from_promote_config`` overrides take precedence; ``None`` falls
    through to the defaults baked into ``CONFIG.promote``."""
    from hammerhead.config import CONFIG

    pc = CONFIG.promote
    cfg = promote.MatchConfig.from_promote_config()
    assert cfg.n_games == pc.default_n_games
    assert cfg.test == pc.default_test

    cfg = promote.MatchConfig.from_promote_config(
        n_games=7, time_ms_per_stone=42, test="wilson"
    )
    assert cfg.n_games == 7
    assert cfg.time_ms_per_stone == 42
    assert cfg.test == "wilson"
    # Untouched fields still come from the config.
    assert cfg.sprt_alpha == pc.sprt_alpha


def test_cli_help_smoke() -> None:
    """``hammerhead match`` and ``hammerhead promote`` parse their argv at all."""
    for sub in ("match", "promote"):
        r = subprocess.run(
            [sys.executable, "-m", "hammerhead.cli", sub, "--help"],
            capture_output=True,
            text=True,
            timeout=10,
        )
        assert r.returncode == 0, f"hammerhead {sub} --help failed: {r.stderr}"
        assert "--time-ms" in r.stdout
        assert "--test" in r.stdout


def test_setup_worktree_idempotent(tmp_path: Path) -> None:
    """Running the script twice in a fresh repo creates exactly one worktree."""
    if not _SETUP_SCRIPT.is_file():
        pytest.skip("setup_worktree.sh not present")

    repo = tmp_path / "fake-repo"
    repo.mkdir()
    env = {**os.environ, "HEXO_SKIP_BUILD": "1"}

    def run(cmd: list[str], **kw: object) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            cmd, cwd=repo, env=env, text=True, capture_output=True, check=True, **kw
        )

    run(["git", "init", "-q"])
    run(["git", "config", "user.email", "t@t"])
    run(["git", "config", "user.name", "t"])
    run(["git", "config", "commit.gpgsign", "false"])
    (repo / "f.txt").write_text("x")
    run(["git", "add", "."])
    run(["git", "commit", "-q", "-m", "init"])

    # First invocation: bootstraps .bestref and creates the worktree.
    r1 = subprocess.run(
        ["bash", str(_SETUP_SCRIPT)],
        cwd=repo,
        env=env,
        text=True,
        capture_output=True,
    )
    assert r1.returncode == 0, f"first run failed: {r1.stderr}"
    assert (repo / ".bestref").is_file()
    assert (repo / ".worktree-best").is_dir()

    # Second invocation: idempotent — should not error or duplicate.
    r2 = subprocess.run(
        ["bash", str(_SETUP_SCRIPT)],
        cwd=repo,
        env=env,
        text=True,
        capture_output=True,
    )
    assert r2.returncode == 0, f"second run failed: {r2.stderr}"

    wt_list = subprocess.check_output(
        ["git", "worktree", "list"], cwd=repo, text=True
    )
    assert wt_list.count(".worktree-best") == 1

    # Clean up the worktree so tmp_path teardown succeeds without git noise.
    subprocess.run(
        ["git", "worktree", "remove", "--force", ".worktree-best"],
        cwd=repo,
        env=env,
        text=True,
        capture_output=True,
        check=False,
    )
    shutil.rmtree(repo / ".worktree-best", ignore_errors=True)
