# Phase 17 STEP 1.1 — Match Harness Structure Scan

This document maps the current `make vs` promotion harness to inform parallelization decisions.

---

## 1. Entry Point

**Makefile target:** `vs` (line 109-112)

```makefile
vs: ## [Phase 11] current vs best, N_GAMES games — does not advance .bestref
	@./scripts/setup_worktree.sh
	@$(VPY) -m hammerhead.cli promote --dry-run \
	    --n $(N_GAMES) --time-ms $(TIME_MS) --test $(TEST)
```

**Call chain:**
1. `make vs` invokes `setup_worktree.sh` (bootstrap)
2. Calls Python: `python -m hammerhead.cli promote --dry-run`
3. CLI entry: `hammerhead/cli.py:cmd_promote()` (line 861)
4. Core harness: `hammerhead.promote.run_match()` (line 416)

**Parameters passed through:**
- `--n N_GAMES` (default 200) → `MatchConfig.n_games`
- `--time-ms TIME_MS` (default 1000) → `MatchConfig.time_ms_per_stone`
- `--test TEST` (default "sprt") → `MatchConfig.test`
- `--dry-run` → skips `.bestref` write (line 908-910)

---

## 2. Game-Loop Structure

### Lifecycle of one game:

**Function:** `play_one_game()` (line 305-360 in promote.py)

```
┌─ play_one_game(a, b, a_is_x, time_ms, max_plies)
│
├─ a.reset()                         # Reset both engines to start (line 319)
├─ b.reset()
│
├─ Loop: while plies < max_plies     # Line 324-345
│  │
│  ├─ Determine to_move side         # Line 325: a.to_move()
│  ├─ Select active mover (a or b)   # Line 326-328
│  │
│  ├─ mover.best_move(time_ms)       # Engine search at time budget (line 330)
│  ├─ mover.place(q, r)              # Apply move locally (line 331)
│  ├─ other.place(q, r)              # Mirror to other engine (line 332)
│  ├─ plies += 1                     # Count ply (line 333)
│  │
│  ├─ Check terminal state           # Line 335-345
│  │  ├─ a.winner() (source of truth)
│  │  ├─ b.winner() (parity check)
│  │  └─ Raise BotProtocolError if mismatch
│  │
│  └─ Break if last_winner != "none"
│
└─ Return GameResult
   ├─ winner: "current" | "best" | None
   ├─ plies: total ply count
   └─ current_was_x: bool (for later scoring)
```

**Key facts:**
- Each game spawns **two SubprocessBot instances** (line 442: `with SubprocessBot(...) as a, SubprocessBot(...) as b`)
- Both are spun up fresh; no engine reuse across games
- Bots are **immediately destroyed** after game ends (context manager exit)
- **Per-game state:**
  - Subprocess handles (stdin/stdout pipes)
  - Engine internal state (board, TT)
  - Game move history (implicit in engine state)
- **Subprocess cost per game:** 2× spawn + banner handshake + 2× graceful quit

---

## 3. Engine Instantiation

### Subprocess Model (NOT in-process)

**Class:** `SubprocessBot` (lines 40-164 in promote.py)

```python
class SubprocessBot:
    def __init__(self, cmd: list[str]) -> None:
        # Spawn a fresh subprocess
        self.proc = subprocess.Popen(
            cmd,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            bufsize=1,  # Line buffered for immediate I/O
        )
        try:
            banner = self._readline()  # Handshake: wait for "<name> bot ready"
        except BotProtocolError:
            self.close()
            raise
        if not banner.endswith("bot ready"):
            # Accept both "hexo bot ready" (old) and "hammerhead bot ready" (new)
            self.close()
            raise BotProtocolError(...)
```

**Startup cost per subprocess:**
1. `subprocess.Popen()` (fork + exec)
2. Wait for banner read from stdout
3. Process is live and ready for commands

**Shutdown:**
```python
def close(self) -> None:
    self.quit()  # Send "quit\n", wait for "bye" drain
    self.proc.wait(timeout=5)  # Graceful reap
    # On timeout: self.proc.kill() + wait(timeout=2)
```

### CLI command:

From `cli.py:_bot_cmd()` (line 799-801):
```python
def _bot_cmd(venv_python: Path) -> list[str]:
    module = _detect_cli_module(venv_python)
    return [str(venv_python), "-m", f"{module}.cli", "bot"]
```

**Actual command spawned:** `python -m hammerhead.cli bot [--tt-size-mb]`

**Each game:** 2 subprocesses, each:
- Fork + exec a Python interpreter
- Import hammerhead engine
- Wait for first command (inherent latency)
- Process I/O via text pipes at 1 byte per second (line buffering)

---

## 4. Per-Game State

**MUST be fresh for each game:**
1. **Engine internal state**
   - Board representation
   - Transposition table (TT) — only cleared by `reset()`
   - Threat cache (threat detection state)
   - RNG seed (deterministic within a match; see `color_balance`)
2. **Subprocess handle**
   - stdin/stdout/stderr pipe file descriptors
   - Process ID (PID)
   - Child process memory context
3. **Game record**
   - Move history (implicit in `GameResult`)
   - Ply count

**Key:** Each `SubprocessBot` instance owns one subprocess and one engine instance. When the bot is destroyed (context manager exit), the subprocess is killed. No engine reuse.

---

## 5. Shared State

**CAN be shared across games (and workers):**
1. **MatchConfig** (frozen dataclass, line 235-277)
   - n_games, time_ms_per_stone, test type, thresholds (elo_low, elo_high, alpha, beta)
   - Read-only; safe to pass by reference
2. **Command strings**
   - `current_cmd` and `best_cmd` (line 417-418 in run_match)
   - Immutable; safe to pass to worker processes
3. **Fixture library (if used)**
   - `_load_fixtures_all()` in benchmark.py (line 135) uses `@lru_cache(maxsize=1)`
   - Cached JSON dict of positions
   - **Note:** lru_cache is PER-PROCESS, not shared. Each worker process will have its own cache.
4. **CONFIG** object
   - Global config (frozen dataclass from hexo.toml)
   - Read-only; safe across processes

---

## 6. Result Aggregation

### Match Accumulation

**Accumulator:** `results: list[GameResult]` (line 435 in promote.py)

Structure: `GameResult` (line 281-286):
```python
@dataclass(frozen=True, slots=True)
class GameResult:
    winner: Optional[str]      # "current" | "best" | None
    plies: int
    current_was_x: bool
```

**Appended after each game:** `results.append(r)` (line 450)

### Aggregation & Verdict

**Function:** `_summarize()` (line 363-406)

```
Inputs:
  - results: list[GameResult]
  - cfg: MatchConfig
  - sprt_verdict: str
  - llr: Optional[float]

Output: MatchResult (line 290-302)
  ├─ games_played: int
  ├─ current_wins: sum(r.winner == "current")
  ├─ best_wins: sum(r.winner == "best")
  ├─ draws: sum(r.winner is None)
  ├─ winrate: (wins + 0.5*draws) / n
  ├─ wilson_lower, wilson_upper: wilson_interval(score, n)
  ├─ sprt_llr: Optional[float]
  ├─ sprt_verdict: str
  ├─ estimated_elo: winrate_to_elo(winrate)
  ├─ estimated_elo_ci: (elo_ci_low, elo_ci_hi)
  └─ final_verdict: "PROMOTE" | "REJECT" | "INCONCLUSIVE"
```

**Verdict rules (line 379-391):**
- If `test == "sprt"` and `sprt_verdict == "accept_h1"` → PROMOTE
- Else if `sprt_verdict == "accept_h0"` → REJECT
- Else → INCONCLUSIVE
- Or if `test == "wilson"`: PROMOTE iff `wilson_lower >= threshold`
- Or if `test == "raw"`: PROMOTE iff `winrate >= threshold`

---

## 7. SPRT Early-Stop Computation

### Running LLR Calculation

**Function:** `sprt_llr()` (line 201-226 in promote.py)

```python
def sprt_llr(
    wins: int,
    draws: int,
    losses: int,
    *,
    elo_low: float,      # H0 threshold (e.g., 0.0)
    elo_high: float,     # H1 threshold (e.g., 5.0)
) -> float:
    """Bernoulli SPRT log-likelihood ratio.
    
    Treats each game as 2 Bernoulli trials:
      win  → 2 successes / 2 trials
      draw → 1 success  / 2 trials
      loss → 0 successes / 2 trials
    """
    p0 = elo_to_winrate(elo_low)
    p1 = elo_to_winrate(elo_high)
    # Clamp to avoid log(0)
    eps = 1e-12
    p0 = min(max(p0, eps), 1.0 - eps)
    p1 = min(max(p1, eps), 1.0 - eps)
    
    successes = 2 * wins + draws
    trials = 2 * (wins + draws + losses)
    failures = trials - successes
    
    return successes * log(p1/p0) + failures * log((1-p1)/(1-p0))
```

### Threshold Bounds

**Function:** `sprt_thresholds()` (line 409-413)

```python
def sprt_thresholds(cfg: MatchConfig) -> tuple[float, float]:
    log_high = math.log((1.0 - cfg.sprt_beta) / cfg.sprt_alpha)
    log_low = math.log(cfg.sprt_beta / (1.0 - cfg.sprt_alpha))
    return log_low, log_high
```

### Early-Stop Loop

**Location:** `run_match()` lines 467-473

```python
if cfg.test == "sprt" and llr is not None:
    if llr >= log_high:
        verdict = "accept_h1"  # H1 accepted: current is stronger
        break
    if llr <= log_low:
        verdict = "accept_h0"  # H0 accepted: best is stronger
        break
```

**Key:** The LLR is **recomputed from scratch after each game** (lines 452-462):
```python
if cfg.test == "sprt":
    wins = sum(1 for x in results if x.winner == "current")
    losses = sum(1 for x in results if x.winner == "best")
    draws = sum(1 for x in results if x.winner is None)
    llr = sprt_llr(wins, draws, losses, elo_low=..., elo_high=...)
```

**For parallelization:** This is a **bottleneck** — to parallelize via `imap_unordered()`, each worker must recompute LLR from partial results and communicate early-stop decisions back to the main process.

---

## 8. Thread/Process-Safety Hazards

### Global Mutable State

**Summary: MINIMAL HAZARDS**

1. **lru_cache in benchmark.py** (line 135: `_load_fixtures_all()`)
   - Cached fixture JSON
   - **NOT thread-safe across processes** (each process has its own interpreter; caches don't share)
   - **Not used in promote.py** (only in benchmark.py for bench_*() functions)
   - No hazard for the match harness

2. **CONFIG object** (from config.py)
   - Frozen dataclass; read-only after init
   - Defined at module load time
   - Safe across threads and processes

3. **Subprocess pipes**
   - Each `SubprocessBot` owns stdin/stdout/stderr of one child
   - No shared handles across instances
   - Safe for concurrent instances (different file descriptors)

4. **No global RNG**
   - No shared `random.Random()` state in promote.py
   - Color balance computed deterministically: `(i % 2 == 0) if cfg.color_balance else True` (line 441)

5. **No temp files with fixed paths**
   - SubprocessBot uses pipes, not named sockets or temp files
   - No collision risk across parallel workers

6. **No fixed ports or network resources**
   - Everything is in-process or subprocess I/O

### Conclusion

**No concurrency hazards identified in promote.py itself.** Each subprocess is independent. The match harness is **safe to parallelize** via multiprocessing or ProcessPoolExecutor.

---

## 9. Ablation Harness

### Existing `bench ablation` command

**Yes, it exists in the CLI** (not as a Makefile target, but as a `bench` subcommand).

**Location:** `cli.py:_bench_ablation()` (line 445-464)

```python
def _bench_ablation(args: argparse.Namespace) -> int:
    """Layer 2 S1/S2 ablation self-play A/B (Phase 16)."""
    r = bench.bench_ablation(
        games=args.games,
        time_per_stone_ms=args.time_ms,
    )
    print(f"ablation: {r.games} games at {r.time_per_stone_ms}ms/stone, S1/S2 vs no-S1/S2")
    print(f"  S1/S2 wins: {r.s1s2_wins} / {r.games} ({r.s1s2_winrate * 100:.1f}%)  ...")
    print(f"  Verdict: {r.verdict}")
    return 0
```

**Argparse wiring:** (line 1076-1081)

```python
bs = bsub.add_parser(
    "ablation",
    help="Layer 2 S1/S2 ablation self-play A/B (Phase 16)",
)
bs.add_argument("--games", type=int, default=50)
bs.add_argument("--time-ms", type=int, default=500)
```

**No Makefile target:**
```
$ grep -n "ablation\|make ablation" Makefile
(no output — no ablation target)
```

### S1/S2 Toggle Mechanism

**Function:** `bench_ablation()` in benchmark.py (line 540-594)

**Game driver:** `_run_ablation_game()` (line 503-537)

```python
def _run_ablation_game(
    time_per_stone_ms: int,
    max_plies: int,
    s1s2_is_x: bool,        # Which side gets S1/S2 enabled
    opening_plies: int,
    rng: random.Random,
) -> Optional[int]:
    bx = Bot(BotConfig(time_per_move_ms=time_per_stone_ms))
    bo = Bot(BotConfig(time_per_move_ms=time_per_stone_ms))
    
    # Engine feature toggle (line 514-520)
    if not hasattr(bx.engine, "set_eval_s1s2"):
        raise RuntimeError(
            "engine built without the eval_s1s2 feature — rebuild with "
            "the default feature set to run the ablation A/B"
        )
    bx.engine.set_eval_s1s2(s1s2_is_x)   # Conditionally enable S1/S2
    bo.engine.set_eval_s1s2(not s1s2_is_x)
    
    # Then run the game with both sides using in-process Bot instances
    ...
```

**Key:**
- **NOT subprocess-based** — uses in-process `Bot` instances
- **Toggle method:** `engine.set_eval_s1s2(bool)` — PyO3 call into Rust
- **Expected behavior:** S1/S2 eval is runtime-selectable, not a compile-time cargo feature
- **Color alternation:** `s1s2_is_x = g % 2 == 0` (line 564 in benchmark.py)

**Result dataclass:** `AblationResult` (line 463-476 in benchmark.py)

```python
@dataclass(frozen=True, slots=True)
class AblationResult:
    games: int
    time_per_stone_ms: int
    opening_plies: int
    s1s2_wins: int
    s1s2_losses: int
    draws: int
    s1s2_winrate: float
    wilson_lo: float
    wilson_hi: float
    verdict: str  # "KEEP" | "DROP" | "INCONCLUSIVE"
```

---

## 10. Parallelization Choice Recommendation

### Context

- **Current structure:** Sequential game loop in `run_match()` (line 440-473)
- **Bottleneck:** Each game is independent, but spawning 2 subprocesses per game adds overhead
- **Early-stop:** SPRT requires running LLR check after every game; cannot batch freely
- **Desired outcome:** Reduce wall-clock time for the match harness

### Recommendation: **(B) ProcessPoolExecutor (or multiprocessing.Pool)**

**Justification:**

1. **Why NOT (C) hand-rolled Process+Queue:**
   - Overkill complexity; the standard lib already solves this
   - No custom signaling or feedback loop needed (early-stop can be computed in main process)

2. **Why NOT (D) asyncio:**
   - Not applicable; the bottleneck is CPU-bound subprocess spawning, not I/O latency
   - Python asyncio doesn't parallelize CPU work across cores

3. **Why (A) multiprocessing.Pool or (B) ProcessPoolExecutor:**
   - Both are equivalent in practice
   - Both support `imap_unordered()` or `map()` with limited worker count
   - Both enable **true parallelism** (bypass GIL via separate processes)
   - Both are battle-tested in production code

4. **Specific to this harness:**
   - **Per-game cost:** ~1-2 seconds (depending on `TIME_MS`)
   - **Per-subprocess overhead:** ~100-500ms (fork + exec + import + banner)
   - **N_GAMES typical:** 200 (default, line 15 in Makefile)
   - **Speedup potential:** ~4-8x on a quad-core machine if we parallelize

### Implementation Pattern

**Use ProcessPoolExecutor with limited workers:**

```python
from concurrent.futures import ProcessPoolExecutor

def play_batch_of_games(worker_id, games_indices, current_cmd, best_cmd, cfg):
    """Worker function: play a batch of games, return list of GameResults."""
    results = []
    for i in games_indices:
        a_is_x = (i % 2 == 0) if cfg.color_balance else True
        with SubprocessBot(current_cmd) as a, SubprocessBot(best_cmd) as b:
            r = play_one_game(a, b, a_is_x=a_is_x, time_ms=cfg.time_ms_per_stone, max_plies=cfg.max_plies)
        results.append((i, r))
    return results

# In run_match(), partition cfg.n_games across N workers:
with ProcessPoolExecutor(max_workers=4) as executor:
    futures = []
    games_per_worker = cfg.n_games // num_workers
    for w in range(num_workers):
        start = w * games_per_worker
        end = start + games_per_worker if w < num_workers - 1 else cfg.n_games
        future = executor.submit(play_batch_of_games, w, range(start, end), current_cmd, best_cmd, cfg)
        futures.append(future)
    
    # Collect results
    all_results = []
    for future in futures:
        batch = future.result()
        all_results.extend(batch)
    
    all_results.sort(key=lambda x: x[0])  # Re-sort by game index
    results = [r for _, r in all_results]
```

### Early-Stop Constraint

**Current SPRT loop:** recomputes LLR after every single game (line 467-473)

**With parallelization:** LLR computation shifts to the main process:
- Collect partial results from workers
- Recompute LLR on accumulated results
- If threshold crossed: signal workers to stop and reap remaining futures

This requires:
1. `imap_unordered()` or polling `executor.as_completed()`
2. Main process monitors LLR; cancels remaining futures if verdict reached
3. Backpressure: don't queue too many games if we might early-stop

---

## 11. Summary Table

| Aspect | Detail |
|--------|--------|
| **Entry point** | Makefile `vs` target → `hammerhead.cli promote --dry-run` |
| **Core function** | `promote.run_match(current_cmd, best_cmd, cfg)` (line 416) |
| **Game function** | `play_one_game(a, b, ...)` (line 305), spawns 2 subprocesses per game |
| **Per-game cost** | ~1-2s (time_ms budget) + ~200ms (subprocess overhead) |
| **Accumulator** | `results: list[GameResult]` (line 435) |
| **Verdict** | `_summarize(results, cfg, verdict, llr)` (line 363) |
| **SPRT LLR** | `sprt_llr(wins, draws, losses, elo_low, elo_high)` (line 201), recomputed per game |
| **Early-stop bounds** | `sprt_thresholds(cfg)` (line 409), checked per game (line 467-473) |
| **Thread safety** | No shared mutable state; safe for multiprocessing |
| **Ablation** | `bench_ablation()` in benchmark.py (line 540), uses `engine.set_eval_s1s2(bool)` toggle |
| **Parallelization** | **Recommend ProcessPoolExecutor with 4-8 workers; partition games by index; recompute LLR in main process** |

---

## References

- Makefile: lines 109-117 (vs/promote targets)
- cli.py: lines 861-945 (cmd_promote entry point)
- promote.py: lines 416-475 (run_match core loop)
- promote.py: lines 305-360 (play_one_game)
- promote.py: lines 40-164 (SubprocessBot class)
- promote.py: lines 201-226 (sprt_llr function)
- promote.py: lines 363-406 (_summarize function)
- benchmark.py: lines 540-594 (bench_ablation)
- benchmark.py: lines 503-537 (_run_ablation_game)

