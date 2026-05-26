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
#
# Override defaults to PGO-build a different source tree (Sprint 2A —
# worktree-PGO opt-in for `make vs` / `make promote`):
#   HEXO_PGO_ROOT       source tree root (default: main repo root)
#   HEXO_PGO_VENV       venv with maturin / python (default:
#                       ${HEXO_PGO_ROOT}/.venv)
#   HEXO_PGO_ENGINE_DIR engine subdir under the root (default:
#                       ${HEXO_PGO_ROOT}/hammerhead-engine)
#   HEXO_PGO_TARGET_DIR cargo target dir for instrumented + optimized
#                       builds (default: ${HEXO_PGO_ENGINE_DIR}/target-pgo)
set -euo pipefail

if [[ "${HEXO_SKIP_PGO:-0}" == "1" ]]; then
  echo "pgo: HEXO_SKIP_PGO=1 set; skipping PGO build"
  exit 0
fi

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# Source tree to PGO-build. Defaults to the main repo so the existing
# `make pgo` path is unchanged; worktree callers override.
PGO_ROOT="${HEXO_PGO_ROOT:-${REPO_ROOT}}"
ENGINE_DIR="${HEXO_PGO_ENGINE_DIR:-${PGO_ROOT}/hammerhead-engine}"
VENV_DIR="${HEXO_PGO_VENV:-${PGO_ROOT}/.venv}"

# Sprint 1B — isolate PGO target dir so the instrumented + optimized
# builds don't pollute the main `target/` (used by `make build`,
# `make bench-iai`, etc.). Cargo respects CARGO_TARGET_DIR via env;
# maturin picks it up transparently.
PGO_TARGET_DIR="${HEXO_PGO_TARGET_DIR:-${ENGINE_DIR}/target-pgo}"
export CARGO_TARGET_DIR="${PGO_TARGET_DIR}"
PGO_DATA="${PGO_TARGET_DIR}/pgo-data"

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

# Locate the python interpreter — prefer the target venv.
PY="${VENV_DIR}/bin/python"
if [[ ! -x "${PY}" ]]; then
  PY="$(command -v python3 || command -v python)"
fi
if [[ -z "${PY}" ]]; then
  echo "pgo: no python interpreter found"
  exit 1
fi

MATURIN="${VENV_DIR}/bin/maturin"
if [[ ! -x "${MATURIN}" ]]; then
  echo "pgo: ${MATURIN} not found; activate the target venv first"
  exit 1
fi

# Training script lives in the source tree we're building, not the
# main repo (matters when PGO-building a worktree at a different SHA).
TRAINING_SCRIPT="${PGO_ROOT}/scripts/pgo_training.py"

echo "pgo: root = ${PGO_ROOT}"
echo "pgo: venv = ${VENV_DIR}"
echo "pgo: data dir = ${PGO_DATA}"
rm -rf "${PGO_DATA}"
mkdir -p "${PGO_DATA}"

# Sprint 4B — maturin develop installs into the venv pointed to by
# VIRTUAL_ENV. When pgo_build.sh runs from setup_worktree.sh AFTER its
# `deactivate`, VIRTUAL_ENV is unset, and maturin falls back to the
# `python` it finds on PATH — which is the OUTER (main) .venv when
# `make vs` invokes the chain. The result: worktree's PGO'd .so gets
# installed into MAIN .venv, silently corrupting the candidate-vs-best
# arena. Force VIRTUAL_ENV to the target venv to pin the install.
export VIRTUAL_ENV="${VENV_DIR}"

echo "pgo: pass 1 — building instrumented engine"
RUSTFLAGS="-Cprofile-generate=${PGO_DATA}" \
  "${MATURIN}" develop --release \
  --manifest-path "${ENGINE_DIR}/Cargo.toml"

echo "pgo: pass 2 — running training workload"
HEXO_PGO_DATA="${PGO_DATA}" "${PY}" "${TRAINING_SCRIPT}"

echo "pgo: pass 3 — merging profiles (${LLVM_PROFDATA})"
"${LLVM_PROFDATA}" merge \
  -o "${PGO_DATA}/merged.profdata" \
  "${PGO_DATA}"/*.profraw

echo "pgo: pass 4 — rebuilding with profile-use"
RUSTFLAGS="-Cprofile-use=${PGO_DATA}/merged.profdata -Cllvm-args=-pgo-warn-missing-function" \
  "${MATURIN}" develop --release \
  --manifest-path "${ENGINE_DIR}/Cargo.toml"

# Sprint 2A — pip 26 skips reinstall when wheel name+version unchanged,
# leaving the previously-installed (instrumented or non-PGO) .so in
# site-packages. Force-overwrite the installed abi3.so with the
# freshly-PGO'd cdylib so the venv runs the optimised binary.
PGO_SO="${PGO_TARGET_DIR}/release/libhammerhead_engine_core.so"
INSTALLED_SO=$(find "${VENV_DIR}/lib" -maxdepth 4 -path '*/site-packages/hammerhead_engine/hammerhead_engine.abi3.so' 2>/dev/null | head -1)
if [[ -f "${PGO_SO}" && -n "${INSTALLED_SO}" ]]; then
  if ! cmp -s "${PGO_SO}" "${INSTALLED_SO}"; then
    echo "pgo: force-overwriting installed abi3.so with PGO'd cdylib"
    cp -f "${PGO_SO}" "${INSTALLED_SO}"
  fi
else
  echo "pgo: WARNING — expected PGO cdylib (${PGO_SO}) or installed abi3.so missing; skipping force-copy"
fi

echo "pgo: done."
