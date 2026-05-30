# NNUE net artifacts

These JSON files are trained leaf-eval networks consumed by
`src/nnue.rs`. The active net is selected in `hexo.toml`
(`[engine.nnue] net_file`); `build.rs` reads it at compile time and
codegens the weights into `config_generated.rs` (no runtime parse, no
`serde_json` dependency — same pattern as the rest of the config).

## `peraxis_aug.json` (active)

The production leaf eval. When `[engine.nnue] enabled = true` it
replaces the hand-built Layer-1/2/3 positional eval; mate / fork /
terminal logic in `eval::eval` still runs first and dominates (the net
output is clamped below `MATE_SCORE - 1000`).

| field        | value |
|--------------|-------|
| architecture | 32-input → 16 hidden (ReLU) → 1, sigmoid-logit |
| feature set  | per-axis open-k histogram (Q/R/S, k=1..5, X/O) + stone counts |
| symmetry     | D6 board-level augmentation (orbit-6, eff. ≈ 5.9×) |
| labels       | human-game OUTCOME (side-to-move eventually won) |
| corpus       | 6,749 human games, 40,452 sampled positions |
| out_scale    | 600 (logit → X-positive centipawn-ish score) |
| inference    | int16 post-training quant (Stockfish scheme) when `quantize=true`, else float |

### Provenance

Trained offline in `diag_nnue/` (branch `diag-nnue-outcome`), Gate A
(feature/aug selection) + Gate B (incremental accumulator + quant).
Feature extraction is `diag_nnue/features2.py`; the Rust per-axis
open-k scan in `nnue.rs::scan_line_openk` mirrors it byte-for-byte.

Forward-pass faithfulness (verified at export):
- Rust float forward vs sklearn `predict_proba`: max|Δp| = 2.5e-4
- int16-quant vs float: max|Δp| = 1.2e-2 (≈ 7 score units at out_scale 600)

Strength (diag Gate C, see `diag_nnue/RESULTS.md` / `STAGE2.md`):
- equal-depth vs hand-built eval: +54 Elo CI[+29,+79] (770 games)
- real-time @500ms vs hand-built eval: +60 Elo CI[+11,+108] (200 games)
- external vs SB-perf: flat (Δ 0.000) — raises absolute strength, does
  not close the structural per-turn gap to SB (known, accepted).

The incremental accumulator (`Accumulator`) maintains the feature
vector in `Board::apply_set` / `apply_clear`; an
`incremental == full-recompute` regression test guards it.
