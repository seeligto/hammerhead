#!/usr/bin/env bash
#
# tools/qsearch_tt_ab.sh — Phase 28F-3.4 A/B measurement.
#
# Toggle `engine.search.qsearch_tt_enabled` between `true` and `false`
# in hexo.toml, rebuild, run `make bench-perf` twice, and emit a JSON
# delta to benches/results/tune/qsearch_tt_28f_3_4.json.
#
# Idempotent: restores the original hexo.toml on exit (even on error).
#
# Usage:
#   tools/qsearch_tt_ab.sh
#
# Reads bench-perf stdout for the canonical NPS / cyc-per-node lines.
# bench-perf must be a fresh, feature-free build (no tt_stats), so the
# script invokes `make build` before each bench run.

set -euo pipefail

REPO_ROOT=$(git rev-parse --show-toplevel)
cd "$REPO_ROOT"

CFG=hexo.toml
OUT_DIR=benches/results/tune
OUT_FILE="$OUT_DIR/qsearch_tt_28f_3_4.json"

mkdir -p "$OUT_DIR"

# Snapshot original hexo.toml so we can restore on exit.
BACKUP=$(mktemp)
cp "$CFG" "$BACKUP"

cleanup() {
    cp "$BACKUP" "$CFG"
    rm -f "$BACKUP"
    # Restore production build (matches the value we just put back).
    make build >/dev/null 2>&1 || true
}
trap cleanup EXIT INT TERM

run_bench() {
    local mode=$1
    local enabled=$2
    # Patch hexo.toml: set qsearch_tt_enabled = <enabled>.
    sed -i "s/^qsearch_tt_enabled = .*/qsearch_tt_enabled = ${enabled}/" "$CFG"
    grep "^qsearch_tt_enabled" "$CFG"
    make build >/dev/null 2>&1
    echo ">>> bench-perf with qsearch_tt_enabled=${enabled}" >&2
    make bench-perf 2>&1 | tee "/tmp/qsearch_tt_ab_${mode}.txt"
}

OFF_OUT=$(run_bench off false)
ON_OUT=$(run_bench on true)

# Extract NPS + cyc/node + depth (final summary lines). bench-perf prints
# one line per fixture×time_ms cell; we just capture the raw text in the
# JSON output for downstream analysis. A future iteration can structure
# this with `bench diff`.
python3 - <<PYEOF > "$OUT_FILE"
import json, pathlib

off = pathlib.Path("/tmp/qsearch_tt_ab_off.txt").read_text()
on  = pathlib.Path("/tmp/qsearch_tt_ab_on.txt").read_text()
print(json.dumps({
    "phase": "28F-3.4",
    "description": "qsearch TT probe + store A/B: enabled vs disabled",
    "off_raw": off,
    "on_raw": on,
}, indent=2))
PYEOF

echo ">>> wrote $OUT_FILE" >&2
