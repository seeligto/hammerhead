#!/usr/bin/env bash
#
# scripts/setup_worktree.sh — Phase 11 promotion harness bootstrap.
#
# Ensures that ".worktree-best/" exists, is checked out at the SHA recorded
# in ".bestref", and has a fully built per-worktree venv at
# ".worktree-best/.venv-best/" with the engine installed in release mode.
#
# Idempotent. On a stale .bestref SHA (worktree HEAD ≠ .bestref), the
# worktree is removed and re-created at the new SHA.
#
# Bootstrap: if .bestref is missing, it is initialized to the current
# repo HEAD. This lets `make vs` run "current vs current" out of the box.

set -euo pipefail

REPO_ROOT=$(git rev-parse --show-toplevel)
cd "$REPO_ROOT"

BESTREF_PATH=".bestref"
WT_PATH=".worktree-best"
VENV_NAME=".venv-best"

# 1. Bootstrap .bestref if absent — point at current HEAD.
if [ ! -f "$BESTREF_PATH" ]; then
    HEAD_SHA=$(git rev-parse HEAD)
    echo "$HEAD_SHA" > "$BESTREF_PATH"
    echo "bootstrap: initialized $BESTREF_PATH → $HEAD_SHA"
fi

BESTREF_SHA=$(tr -d '[:space:]' < "$BESTREF_PATH")
if [ -z "$BESTREF_SHA" ]; then
    echo "error: $BESTREF_PATH is empty" >&2
    exit 1
fi

# 2. Validate the SHA exists locally.
if ! git cat-file -e "${BESTREF_SHA}^{commit}" 2>/dev/null; then
    echo "error: $BESTREF_PATH points at $BESTREF_SHA, which is not a commit in this repo" >&2
    exit 1
fi

# 3. Create / refresh worktree.
#    Clean up any dangling registration (worktree dir was deleted manually).
git worktree prune >/dev/null 2>&1 || true

NEEDS_FRESH=0
if [ -d "$WT_PATH" ]; then
    WT_SHA=$(git -C "$WT_PATH" rev-parse HEAD 2>/dev/null || echo "")
    if [ "$WT_SHA" != "$BESTREF_SHA" ]; then
        echo "stale worktree: $WT_SHA != $BESTREF_SHA, recreating"
        git worktree remove --force "$WT_PATH"
        NEEDS_FRESH=1
    fi
else
    NEEDS_FRESH=1
fi

if [ "$NEEDS_FRESH" -eq 1 ]; then
    git worktree add --detach "$WT_PATH" "$BESTREF_SHA"
fi

# 4. Per-worktree venv + maturin build (idempotent).
#    HEXO_SKIP_BUILD=1 short-circuits the build (used by tests).
if [ "${HEXO_SKIP_BUILD:-0}" = "1" ]; then
    echo "worktree ready (build skipped): $WT_PATH @ $BESTREF_SHA"
    exit 0
fi

cd "$WT_PATH"
if [ ! -d "$VENV_NAME" ]; then
    python3 -m venv "$VENV_NAME"
    "$VENV_NAME/bin/pip" install -q -U pip maturin
fi

# Maturin / pip pick up VIRTUAL_ENV from the parent shell (eg. when invoked
# from `make`). Clear it so the worktree's venv is the target instead of the
# outer one. We also activate the worktree venv so PATH is set as expected.
unset VIRTUAL_ENV PYTHONHOME
# shellcheck source=/dev/null
. "$VENV_NAME/bin/activate"

# The worktree may sit at a pre-rename SHA (engine dir `hexo-engine`,
# package dir `hexo`) or a current one (`hammerhead-engine` /
# `hammerhead`). Resolve whichever the checked-out tree actually has.
if [ -d hammerhead-engine ]; then ENGINE_DIR=hammerhead-engine; else ENGINE_DIR=hexo-engine; fi
if [ -d hammerhead ]; then PKG_DIR=hammerhead; else PKG_DIR=hexo; fi

cd "$ENGINE_DIR"
maturin develop --release --quiet
cd ..
pip install -q -e "$PKG_DIR"

deactivate

# Sprint 2A — optional PGO retrain inside the worktree's venv. Driven
# by the same `pgo_build.sh` pipeline as the main repo, parameterised
# via HEXO_PGO_* env vars. Default off (preserves the fast path for
# tests / dev iteration); `make vs` / `make promote` set HEXO_PGO=1
# for apples-to-apples arena measurement.
if [ "${HEXO_PGO:-0}" = "1" ]; then
    echo "worktree: HEXO_PGO=1 — retraining PGO inside $WT_PATH"
    HEXO_PGO_ROOT="$(pwd)" \
    HEXO_PGO_VENV="$(pwd)/$VENV_NAME" \
    HEXO_PGO_ENGINE_DIR="$(pwd)/$ENGINE_DIR" \
        bash "$REPO_ROOT/scripts/pgo_build.sh"
fi

echo "worktree ready: $WT_PATH @ $BESTREF_SHA"
