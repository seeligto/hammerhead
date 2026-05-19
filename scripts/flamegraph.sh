#!/usr/bin/env bash
# Capture a release-profile flamegraph of `bench_search` and drop the
# SVG in `benches/results/` for inspection. Phase 12 hygiene tool.
#
# Requirements (install once):
#   cargo install flamegraph     # the cargo-flamegraph subcommand
#   sudo pacman -S perf          # Linux kernel profiler (Arch)
#                                # — or your distro's `linux-tools` /
#                                #   `linux-perf` equivalent.
#   echo 1 | sudo tee /proc/sys/kernel/perf_event_paranoid
#                                # — only required without root.
#
# On macOS, cargo-flamegraph uses dtrace; install Xcode CLT and the
# script falls through unchanged.

set -euo pipefail

SHA=$(git rev-parse --short HEAD)
DATE=$(date +%Y-%m-%dT%H-%M-%S)
REPO_ROOT=$(git rev-parse --show-toplevel)
OUT="${REPO_ROOT}/benches/results/flamegraph-${DATE}-${SHA}.svg"

mkdir -p "${REPO_ROOT}/benches/results"

cd "${REPO_ROOT}/hexo-engine"
cargo flamegraph --release \
    --bench bench_search \
    --output "${OUT}" \
    -- --bench

echo "wrote ${OUT}"
