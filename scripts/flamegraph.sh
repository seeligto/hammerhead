#!/usr/bin/env bash
# Capture a release-profile flamegraph of `bench_search`. Produces two
# files in `benches/results/`:
#   flamegraph-<date>-<sha>.svg          — interactive SVG, open in a browser
#   flamegraph-<date>-<sha>.folded.txt   — collapsed stacks, one line each,
#                                          readable in any text editor or LLM
#
# Phase 12 hygiene tool. The .folded.txt format is `frame_a;frame_b;... N`
# where N is the sample count; the top entries are the hottest paths.
#
# Requirements (install once):
#   cargo install flamegraph    # cargo-flamegraph subcommand (already in repo)
#   cargo install inferno       # inferno-collapse-perf for folded stacks
#   sudo pacman -S perf         # Arch: perf userspace tools
#                               # Debian/Ubuntu: linux-tools-common +
#                               #   linux-tools-$(uname -r)
#   echo 1 | sudo tee /proc/sys/kernel/perf_event_paranoid
#                               # Permanent: add to /etc/sysctl.d/

set -euo pipefail

SHA=$(git rev-parse --short HEAD)
DATE=$(date +%Y-%m-%dT%H-%M-%S)
REPO_ROOT=$(git rev-parse --show-toplevel)
RESULTS="${REPO_ROOT}/benches/results"
SVG="${RESULTS}/flamegraph-${DATE}-${SHA}.svg"
FOLDED="${RESULTS}/flamegraph-${DATE}-${SHA}.folded.txt"
PERF_DATA="${RESULTS}/.flamegraph.perf.data"

mkdir -p "${RESULTS}"

# Pre-flight: surface missing tools clearly instead of mid-perf failures.
for tool in perf cargo-flamegraph; do
    if ! command -v "${tool}" >/dev/null 2>&1; then
        echo "error: ${tool} not in PATH — see header of this script for install steps" >&2
        exit 1
    fi
done

# cargo-flamegraph keeps perf.data when --perfdata is supplied; we need it
# anyway to emit the folded-stack text, so direct it into results/.
cd "${REPO_ROOT}/hexo-engine"
PERF=perf cargo flamegraph --release \
    --bench bench_search \
    --output "${SVG}" \
    --perfdata "${PERF_DATA}" \
    -- --bench

# Folded stacks for grep / LLM analysis. inferno-collapse-perf parses the
# perf-script output and sorts hottest-first by sample count.
perf script -i "${PERF_DATA}" \
    | "${HOME}/.cargo/bin/inferno-collapse-perf" \
    | sort -t' ' -k2 -nr \
    > "${FOLDED}"

# perf.data is large and binary; keep folded text and SVG only.
rm -f "${PERF_DATA}" "${PERF_DATA}.old"

echo "wrote ${SVG}"
echo "wrote ${FOLDED}"
echo
echo "Top 10 hottest frames:"
head -10 "${FOLDED}"
