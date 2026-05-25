"""Phase 28F-2 sub-phase 0 — eval comparison harness (TEMPORARY).

DIAGNOSTIC TOOL. Not engine code. Safe to remove after Phase 28F-2.

Replays HH-loss games from the 28F-1 corpus, queries HH and SB-perf at
the decisive position plus three earlier samples, and emits CSV with
per-side eval, depth reached, and HH layer breakdown.

Inputs:
  - /tmp/phase_28f/1/A/classification.csv (192 HH losses, categorised)
  - ~/Work/hexo-arena/runs/{e0-verify-hh500-sb500, e0-condA-hh500-sb500,
    d2-i4-hh-vs-sbperf-50g}/games/*.json

Outputs:
  - /tmp/phase_28f/2/0/eval_comparison.csv

HH eval is read via Bot._engine._debug_eval_layers (TEMP method) at
depth-1. SB-perf eval is read via MinimaxBot.last_score after a
depth-1 search at 10ms time limit.
"""

from __future__ import annotations

import csv
import json
import os
import sys
import time
from pathlib import Path

# ── path wiring (env-tolerant; respects HAMMERHEAD_REPO / SEALBOT_REPO) ──
HH_REPO = Path(os.environ.get("HAMMERHEAD_REPO", str(Path.home() / "Work" / "hammerhead"))).resolve()
SB_REPO = Path(os.environ.get(
    "SEALBOT_REPO",
    str(Path.home() / "Work" / "hexo-arena" / "external" / "sealbot_perf"),
)).resolve()
ARENA_RUNS = Path(os.environ.get(
    "ARENA_RUNS_DIR",
    str(Path.home() / "Work" / "hexo-arena" / "runs"),
)).resolve()
CLASSIFICATION_CSV = Path(os.environ.get(
    "CLASSIFICATION_CSV",
    "/tmp/phase_28f/1/A/classification.csv",
)).resolve()
OUTPUT_CSV = Path(os.environ.get(
    "OUTPUT_CSV",
    "/tmp/phase_28f/2/0/eval_comparison.csv",
)).resolve()

# Insert hammerhead Python package onto path so `from hammerhead import Bot`
# finds the editable install's source (the .venv install registers it,
# but running outside the venv still needs this fallback).
sys.path.insert(0, str(HH_REPO / "hammerhead"))
sys.path.insert(0, str(SB_REPO / "current"))
sys.path.insert(0, str(SB_REPO))

from hammerhead import Bot  # type: ignore  # noqa: E402
from game import HexGame, Player  # type: ignore  # noqa: E402
from minimax_cpp import MinimaxBot  # type: ignore  # noqa: E402

# ── arena run directories holding the 192 games ──
RUN_DIRS = [
    ARENA_RUNS / "e0-verify-hh500-sb500",
    ARENA_RUNS / "e0-condA-hh500-sb500",
    ARENA_RUNS / "d2-i4-hh-vs-sbperf-50g",
]

# Per-position search budget. depth=1 is the gate; the tiny time bound
# is a safety belt for SB-perf which is iterative-deepening native.
HH_DEPTH = 1
HH_TIME_MS = None  # depth-only
SB_DEPTH = 1
SB_TIME_S = 0.05


def opening_player_sequence(n: int) -> list[str]:
    """Player sequence for the first `n` opening stones (HeXO rule:
    turn 1 = 1 X-stone, turns ≥2 = 2 stones alternating starting O)."""
    out: list[str] = []
    i = 0
    turn = 1
    stones_in_turn = 1
    player = "X"
    while i < n:
        for _ in range(stones_in_turn):
            if i >= n:
                break
            out.append(player)
            i += 1
        turn += 1
        stones_in_turn = 2
        player = "O" if player == "X" else "X"
    return out


def load_game(path: Path) -> dict | None:
    """Load a game JSON and tag the HH side. Returns None if neither
    bot is Hammerhead."""
    with open(path) as f:
        g = json.load(f)
    bx = g["bot_x"]["name"]
    bo = g["bot_o"]["name"]
    if bx == "Hammerhead":
        hh_side = "X"
    elif bo == "Hammerhead":
        hh_side = "O"
    else:
        return None
    g["_hh_side"] = hh_side
    return g


def all_stones(game: dict) -> list[tuple[tuple[int, int], str]]:
    """Flat (coord, player) sequence: opening stones in their forced
    X/O assignment, then the played stones (each with its own player
    tag from the JSON)."""
    out: list[tuple[tuple[int, int], str]] = []
    opening = game.get("opening_moves") or []
    op_players = opening_player_sequence(len(opening))
    for coord, p in zip(opening, op_players):
        out.append((tuple(coord), p))
    for s in game["stones"]:
        out.append((tuple(s["coord"]), s["player"]))
    return out


def replay_into_hh_bot(bot: Bot, stones: list[tuple[tuple[int, int], str]], up_to: int) -> bool:
    """Replay the first `up_to` stones into a fresh HH Bot. Returns
    True if the bot is left in a non-terminal state; False if the game
    ended at or before `up_to`."""
    bot.reset()
    for i, (coord, _player) in enumerate(stones[:up_to]):
        try:
            bot.play(coord)
        except Exception as exc:
            sys.stderr.write(f"  [hh-replay] failed at stone {i}: {exc}\n")
            return False
        if bot.is_game_over:
            return False
    return True


def build_sb_game(stones: list[tuple[tuple[int, int], str]], up_to: int) -> tuple[HexGame, str] | None:
    """Replay `up_to` stones into a SealBot HexGame. Returns
    (game, side_to_move_arena) or None if game ended."""
    g = HexGame(win_length=6)
    for coord, player in stones[:up_to]:
        # Map arena 'X'/'O' to SealBot Player.A / Player.B.
        sb_player = Player.A if player == "X" else Player.B
        # Force the current_player on each move (SealBot's make_move
        # uses self.current_player to place; we override per-stone to
        # respect the historical sequence regardless of SealBot's
        # internal turn-tracker).
        g.current_player = sb_player
        ok = g.make_move(coord[0], coord[1])
        if not ok:
            return None
        if g.game_over:
            return None
        # Reset moves_left so each loop iteration plants exactly the
        # stone we want (don't let SealBot's internal switch confuse
        # us — we set current_player explicitly above).
        if g.moves_left_in_turn <= 0:
            g.moves_left_in_turn = 1
    # Side-to-move = whoever plays the next stone in the real history.
    if up_to >= len(stones):
        return None
    next_player_arena = stones[up_to][1]
    g.current_player = Player.A if next_player_arena == "X" else Player.B
    # moves_left_in_turn = 1 if next stone starts a new turn, 2 if mid-turn.
    # Inferred by counting consecutive same-player stones backwards.
    moves_left = 1
    if up_to >= 1 and stones[up_to - 1][1] == next_player_arena:
        moves_left = 1  # second stone of same turn
    else:
        moves_left = 2 if up_to >= 1 else 1
    g.moves_left_in_turn = moves_left
    g.move_count = up_to
    return g, next_player_arena


# ── SB-perf eval query ──
class SBProbe:
    """Holds a single MinimaxBot instance; queries depth-1 eval."""

    def __init__(self):
        self.bot = MinimaxBot(SB_TIME_S)
        # Cap depth to keep the call shallow even if time allows more.
        try:
            self.bot.max_depth = SB_DEPTH
        except Exception:
            pass

    def eval_at(self, game: HexGame) -> tuple[float | None, int | None]:
        """Run a depth-1 search; return (last_score, last_depth).
        last_score is from SB's *current_player* perspective."""
        self.bot.time_limit = SB_TIME_S
        try:
            self.bot.max_depth = SB_DEPTH
        except Exception:
            pass
        try:
            moves = self.bot.get_move(game)
            # exhaust generator if needed
            if hasattr(moves, "__next__"):
                last = None
                for m in moves:
                    last = m
                moves = last or []
            else:
                moves = list(moves)
        except Exception as exc:
            sys.stderr.write(f"  [sb-eval] failed: {exc}\n")
            return None, None
        score = float(getattr(self.bot, "last_score", 0.0) or 0.0)
        depth = int(getattr(self.bot, "last_depth", 0) or 0)
        return score, depth


def hh_eval_at(bot: Bot, stones: list[tuple[tuple[int, int], str]], up_to: int):
    """Query HH at depth=1 from the replayed position. Returns dict
    with hh_eval, hh_depth, layer1, layer2, layer3."""
    if not replay_into_hh_bot(bot, stones, up_to):
        return None
    try:
        _, stats = bot.suggest(depth=HH_DEPTH, return_stats=True)
    except Exception as exc:
        sys.stderr.write(f"  [hh-suggest] failed: {exc}\n")
        return None
    l1, l2, l3 = bot._engine._debug_eval_layers()
    return {
        "hh_eval": int(stats.score),
        "hh_depth": int(stats.max_depth_reached),
        "layer1": int(l1),
        "layer2": int(l2),
        "layer3": int(l3),
    }


def sample_positions(game: dict, decisive_stone_idx: int) -> list[tuple[int, str]]:
    """Return list of (stone-count-replayed, label) sample positions.
    Decisive = HH's decisive stone index from classification CSV (1-based
    into game['stones'])."""
    total = len(game["stones"])
    opening_n = len(game.get("opening_moves") or [])
    # decisive_stone_idx (1-based) -> replay this many played stones
    # (we evaluate the position AFTER that stone is placed).
    decisive = max(1, min(decisive_stone_idx, total))
    # Earlier samples: stone 10/15/20 measured by total stones placed
    # (opening + played). Fallback to 0.4/0.6/0.8 of game length.
    full = opening_n + total
    if full >= 22:
        targets = [10, 15, 20]
    else:
        targets = [max(2, int(full * 0.4)),
                   max(3, int(full * 0.6)),
                   max(4, int(full * 0.8))]
    out: list[tuple[int, str]] = []
    for t, label in zip(targets, ["stone10", "stone15", "stone20"]):
        # `t` is total stones (opening included). Convert to a played
        # stone count: played = max(0, t - opening_n).
        played = max(1, t - opening_n)
        played = min(played, total - 1)
        out.append((played, label))
    out.append((decisive, "decisive"))
    # Dedup, preserve order.
    seen = set()
    deduped = []
    for cnt, lbl in out:
        if cnt in seen:
            continue
        seen.add(cnt)
        deduped.append((cnt, lbl))
    return deduped


def main():
    OUTPUT_CSV.parent.mkdir(parents=True, exist_ok=True)

    # Load classification CSV → dict keyed by game_id (e.g. "e0-verify/p000g0").
    classifications: dict[str, dict] = {}
    with open(CLASSIFICATION_CSV) as f:
        rdr = csv.DictReader(f)
        for row in rdr:
            classifications[row["game_id"]] = row

    # Build {game_id → full file path}. The CSV's `source` tag maps to
    # the run directory shortcut.
    source_to_dir = {
        "e0-verify": ARENA_RUNS / "e0-verify-hh500-sb500",
        "e0-condA": ARENA_RUNS / "e0-condA-hh500-sb500",
        "d2-i4": ARENA_RUNS / "d2-i4-hh-vs-sbperf-50g",
    }

    bot = Bot(time_per_stone_ms=200)
    sb = SBProbe()

    rows = []
    fieldnames = [
        "game_id", "category", "position_idx", "position_label",
        "stone_count", "side_to_move", "hh_eval", "sb_eval",
        "hh_depth_reached", "sb_depth_reached", "outcome",
        "hh_layer1", "hh_layer2", "hh_layer3",
    ]

    t0 = time.time()
    n_games = 0
    n_positions = 0
    n_errors = 0
    for game_id, cls in sorted(classifications.items()):
        source = cls["source"]
        pair_idx = int(cls["pair_idx"])
        game_in_pair = int(cls["game_in_pair"])
        decisive_idx = int(cls["decisive_move_idx"])  # 1-based stone index
        category = cls["category"]
        hh_side = cls["hh_side"]
        winner = cls["winner"]
        run_dir = source_to_dir.get(source)
        if run_dir is None:
            sys.stderr.write(f"  [skip] unknown source: {source}\n")
            continue
        path = run_dir / "games" / f"pair_{pair_idx:05d}_game_{game_in_pair}.json"
        if not path.is_file():
            sys.stderr.write(f"  [skip] missing file: {path}\n")
            continue
        game = load_game(path)
        if game is None:
            continue
        stones = all_stones(game)
        opening_n = len(game.get("opening_moves") or [])

        samples = sample_positions(game, decisive_idx)
        n_games += 1
        for i, (played_cnt, label) in enumerate(samples):
            up_to = opening_n + played_cnt
            if up_to >= len(stones):
                continue
            side_to_move = stones[up_to][1]
            hh_data = hh_eval_at(bot, stones, up_to)
            if hh_data is None:
                n_errors += 1
                continue
            sb_state = build_sb_game(stones, up_to)
            sb_eval = None
            sb_depth = None
            if sb_state is not None:
                sb_game, sb_stm = sb_state
                ev, dp = sb.eval_at(sb_game)
                if ev is not None:
                    # Normalise: SB's last_score is from its current_player
                    # perspective. We want X-positive globally to match HH.
                    sign = +1 if sb_stm == "X" else -1
                    sb_eval = sign * ev
                    sb_depth = dp
            rows.append({
                "game_id": game_id,
                "category": category,
                "position_idx": i,
                "position_label": label,
                "stone_count": up_to,
                "side_to_move": side_to_move,
                "hh_eval": hh_data["hh_eval"],
                "sb_eval": sb_eval,
                "hh_depth_reached": hh_data["hh_depth"],
                "sb_depth_reached": sb_depth,
                "outcome": winner,
                "hh_layer1": hh_data["layer1"],
                "hh_layer2": hh_data["layer2"],
                "hh_layer3": hh_data["layer3"],
            })
            n_positions += 1
        if n_games % 20 == 0:
            elapsed = time.time() - t0
            sys.stderr.write(
                f"  [progress] {n_games} games, {n_positions} positions, "
                f"{n_errors} errors, {elapsed:.1f}s\n",
            )

    with open(OUTPUT_CSV, "w", newline="") as f:
        wr = csv.DictWriter(f, fieldnames=fieldnames)
        wr.writeheader()
        wr.writerows(rows)

    elapsed = time.time() - t0
    sys.stderr.write(
        f"DONE: {n_games} games, {n_positions} positions, {n_errors} errors, "
        f"{elapsed:.1f}s → {OUTPUT_CSV}\n",
    )


if __name__ == "__main__":
    main()
