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

# `cargo install`'d binaries live in ~/.cargo/bin, which isn't always in
# the PATH of non-login shells (e.g. CI, make).
export PATH="${HOME}/.cargo/bin:${PATH}"

SHA=$(git rev-parse --short HEAD)
DATE=$(date +%Y-%m-%dT%H-%M-%S)
REPO_ROOT=$(git rev-parse --show-toplevel)
RESULTS="${REPO_ROOT}/benches/results"
SVG="${RESULTS}/flamegraph-${DATE}-${SHA}.svg"
FOLDED="${RESULTS}/flamegraph-${DATE}-${SHA}.folded.txt"
PERF_DATA="${RESULTS}/.flamegraph.perf.data"

mkdir -p "${RESULTS}"

# Pre-flight: surface missing tools clearly instead of mid-perf failures.
for tool in perf cargo-flamegraph inferno-flamegraph inferno-collapse-perf; do
    if ! command -v "${tool}" >/dev/null 2>&1; then
        echo "error: ${tool} not in PATH — see header of this script for install steps" >&2
        exit 1
    fi
done

# cargo-flamegraph doesn't expose the intermediate perf.data, so we drive
# perf + inferno directly. This also lets us emit both the SVG and the
# folded-stack text without re-running the workload.
cd "${REPO_ROOT}/hammerhead-engine"

# Build the bench binary with frame pointers. `perf --call-graph fp` walks
# the frame-pointer chain — depth-unlimited and CFI-independent — whereas
# perf's `dwarf` unwinder cannot follow the LTO'd recursive search: it runs
# out of captured stack mid-recursion (even at a 64 KiB dump) and collapses
# every search sample into an unattributable `[unknown]` / libc leaf, so
# only shallow setup code (Engine::new, clear_tt) is ever attributed.
# `target-cpu=native` is re-stated explicitly because a `RUSTFLAGS` env var
# overrides `.cargo/config.toml` wholesale rather than appending to it.
RUSTFLAGS="-C target-cpu=native -C force-frame-pointers=yes" \
    cargo bench --bench bench_search --no-run >/dev/null

# Pick the most recent bench_search executable in target/release/deps.
BENCH_BIN=$(ls -t target/release/deps/bench_search-* 2>/dev/null \
    | grep -v '\.d$' \
    | head -1)
if [ -z "${BENCH_BIN}" ]; then
    echo "error: bench_search binary not found" >&2
    exit 1
fi

# Record samples. `--call-graph fp` walks the frame-pointer chain built
# into the binary above. `-F 997` is the standard non-aliasing rate.
perf record \
    --call-graph fp \
    -F 997 \
    -o "${PERF_DATA}" \
    --quiet \
    -- "${BENCH_BIN}" --bench >/dev/null

# Emit folded stacks first (hottest path on top), then render to SVG.
perf script -i "${PERF_DATA}" \
    | inferno-collapse-perf \
    | sort -t' ' -k2 -nr \
    > "${FOLDED}"

inferno-flamegraph < "${FOLDED}" > "${SVG}"

# perf.data is large + binary; keep only folded text + SVG.
rm -f "${PERF_DATA}" "${PERF_DATA}.old"

echo "wrote ${SVG}"
echo "wrote ${FOLDED}"
echo
echo "Top 10 hottest frames (count / stack):"
head -10 "${FOLDED}"
