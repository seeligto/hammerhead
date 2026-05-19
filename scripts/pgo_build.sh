#!/usr/bin/env bash
# Phase 14 STEP 9 — Profile-Guided Optimization build pipeline.
#
# Three passes:
#   1. Instrumented build: emit `.profraw` files when the binary runs.
#   2. Training: `pgo_training.py` runs a representative search.
#   3. Optimized build: re-link using the merged profile.
#
# Requires llvm-profdata (Arch: `pacman -S llvm`). Doesn't need
# `cargo-pgo`; the script hand-rolls the `-Cprofile-*` rustflags.
#
# Usage (from repo root):
#   ./scripts/pgo_build.sh
#
# Set HEXO_SKIP_PGO=1 to short-circuit (useful when iterating on
# unrelated build flags).
set -euo pipefail

if [[ "${HEXO_SKIP_PGO:-0}" == "1" ]]; then
  echo "pgo: HEXO_SKIP_PGO=1 set; skipping PGO build"
  exit 0
fi

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PGO_DATA="${REPO_ROOT}/hexo-engine/target/pgo"

# Sanity-check toolchain. Prefer rustup's bundled llvm-profdata when
# available — its format version matches rustc's bundled LLVM, which
# the system llvm-profdata (Arch /usr/bin) is not guaranteed to track.
RUSTUP_LLVM_PROFDATA="$(rustc --print sysroot)/lib/rustlib/$(rustc -vV | awk '/host:/ {print $2}')/bin/llvm-profdata"
if [[ -x "${RUSTUP_LLVM_PROFDATA}" ]]; then
  LLVM_PROFDATA="${RUSTUP_LLVM_PROFDATA}"
elif command -v llvm-profdata >/dev/null; then
  LLVM_PROFDATA="$(command -v llvm-profdata)"
  echo "pgo: warning — falling back to system llvm-profdata; if rustc rejects"
  echo "      the merged profile, run \`rustup component add llvm-tools-preview\`"
else
  echo "pgo: llvm-profdata not found; run \`rustup component add llvm-tools-preview\`"
  exit 1
fi

# Locate the python interpreter — prefer the project venv.
PY="${REPO_ROOT}/.venv/bin/python"
if [[ ! -x "${PY}" ]]; then
  PY="$(command -v python3 || command -v python)"
fi
if [[ -z "${PY}" ]]; then
  echo "pgo: no python interpreter found"
  exit 1
fi

MATURIN="${REPO_ROOT}/.venv/bin/maturin"
if [[ ! -x "${MATURIN}" ]]; then
  echo "pgo: ${MATURIN} not found; activate the project venv first"
  exit 1
fi

echo "pgo: data dir = ${PGO_DATA}"
rm -rf "${PGO_DATA}"
mkdir -p "${PGO_DATA}"

echo "pgo: pass 1 — building instrumented engine"
RUSTFLAGS="-Cprofile-generate=${PGO_DATA}" \
  "${MATURIN}" develop --release \
  --manifest-path "${REPO_ROOT}/hexo-engine/Cargo.toml"

echo "pgo: pass 2 — running training workload"
HEXO_PGO_DATA="${PGO_DATA}" "${PY}" "${REPO_ROOT}/scripts/pgo_training.py"

echo "pgo: pass 3 — merging profiles (${LLVM_PROFDATA})"
"${LLVM_PROFDATA}" merge \
  -o "${PGO_DATA}/merged.profdata" \
  "${PGO_DATA}"/*.profraw

echo "pgo: pass 4 — rebuilding with profile-use"
RUSTFLAGS="-Cprofile-use=${PGO_DATA}/merged.profdata -Cllvm-args=-pgo-warn-missing-function" \
  "${MATURIN}" develop --release \
  --manifest-path "${REPO_ROOT}/hexo-engine/Cargo.toml"

echo "pgo: done."
