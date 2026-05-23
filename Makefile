.DEFAULT_GOAL := help
.PHONY: help build build-tt-stats clean rebuild test lint fmt check vs \
        promote install bench bench-quick bench-perf bench-micro \
        bench-micro-quick bench-diff bench-baseline flamegraph pgo \
        tune tune-smoke

ENGINE    := hammerhead-engine
PY        := hammerhead
VENV      := .venv
VPY       := $(VENV)/bin/python
VPYTEST   := $(VENV)/bin/pytest
VMATURIN  := $(VENV)/bin/maturin

# Phase 11 (promotion harness) defaults — override on the command line:
#   make vs N_GAMES=500 TIME_MS=2000 TEST=wilson N_WORKERS=14
N_GAMES   ?= 200
TIME_MS   ?= 1000
TEST      ?= sprt
ELO_LOW   ?= 0
ELO_HIGH  ?= 5
# Phase 17 parallel match harness — 0 = auto (cpu_count() - 2):
N_WORKERS ?= 0

# Phase 10 (benchmark suite) defaults — override on the command line:
#   make bench BENCH_TIME_MS=2000
#   make bench-micro TARGET=board
#   make bench-diff A=baseline B=20260519-103022-abc1234
BENCH_TIME_MS ?= 1000
TARGET        ?= all

help: ## show available targets
	@echo "Hammerhead — Makefile targets:"
	@grep -E '^[a-zA-Z_-]+:.*?## ' $(MAKEFILE_LIST) \
	  | awk -F':.*?## ' '{printf "  \033[36m%-10s\033[0m %s\n", $$1, $$2}'

build: ## maturin develop --release + pip install -e hammerhead (uses .venv)
	cd $(ENGINE) && $(abspath $(VMATURIN)) develop --release
	$(VPY) -m pip install -e $(PY)

build-tt-stats: ## maturin develop --release --features tt_stats (TT counters for bench)
	cd $(ENGINE) && $(abspath $(VMATURIN)) develop --release --features tt_stats
	$(VPY) -m pip install -e $(PY)

clean: ## remove all build artifacts (target/, __pycache__, *.so, dist/, egg-info)
	-cd $(ENGINE) && cargo clean
	@find . -type d -name __pycache__ -prune -exec rm -rf {} +
	@find . -type d -name '.pytest_cache' -prune -exec rm -rf {} +
	@find . -type f -name '*.so' -delete
	@find . -type f -name '*.pyd' -delete
	@rm -rf $(ENGINE)/dist $(ENGINE)/build
	@rm -rf $(PY)/build $(PY)/dist
	@find $(PY) -type d -name '*.egg-info' -prune -exec rm -rf {} +

rebuild: clean build ## clean + build

test: ## cargo test --release + pytest (uses .venv)
	cd $(ENGINE) && cargo test --release
	cd $(PY) && $(abspath $(VPYTEST))

lint: ## clippy with pedantic lints
	cd $(ENGINE) && cargo clippy --all-targets -- \
	  -D warnings \
	  -W clippy::all \
	  -W clippy::pedantic \
	  -A clippy::module_name_repetitions

fmt: ## cargo fmt
	cd $(ENGINE) && cargo fmt

check: lint test ## lint + test (CI gate)

# ──────────────────────────────────────────────────────────────────────────────
# Phase 10 — benchmark suite (criterion + Python macro-benches).
# See specs/SPEC_BENCHMARKS.md.
# ──────────────────────────────────────────────────────────────────────────────

# `bench` / `bench-baseline` rebuild the engine with --features tt_stats
# first, so the loaded extension carries the TT probe/hit counters and
# baseline.json gets a non-null tt_hit_rate. NOTE: this leaves a
# tt_stats build installed — run `make build` afterwards to restore the
# feature-free production build before any NPS measurement (bench-perf /
# bench-quick), since the tt_stats build is marginally slower.
bench: build-tt-stats ## full sweep (tt_stats build), write canonical JSON to benches/results/
	@$(VPY) -m hammerhead.cli bench all --time-ms $(BENCH_TIME_MS) --tt-stats

# bench-quick / bench-perf measure per-call NPS and intentionally use
# whatever `make build` installed (feature-free). Run `make build`
# before these if a prior `make bench` left a tt_stats build installed.
bench-quick: ## [Phase 16] inner-loop NPS+depth+cyc/node check (~5-15s)
	@$(VPY) -m hammerhead.cli bench quick

bench-perf: ## [Phase 16] two-fixture × multi-budget NPS+cyc/node (~30-60s)
	@$(VPY) -m hammerhead.cli bench perf

bench-micro: ## criterion benches for one TARGET (default: all) + drain
	@cd $(ENGINE) && cargo bench --bench bench_$(TARGET)
	@cd $(ENGINE) && cargo build --release --example bench_drain
	@$(ENGINE)/target/release/examples/bench_drain \
	    --criterion-dir $(ENGINE)/target/criterion

bench-micro-quick: ## [Phase 16] fast criterion for one TARGET (~5-10s, no drain)
	@cd $(ENGINE) && cargo bench --bench bench_$(TARGET) -- \
	    --sample-size 10 --measurement-time 1 --warm-up-time 0.5

bench-diff: ## diff two run JSONs (use A= and B=, names resolved under benches/results/)
	@$(VPY) -m hammerhead.cli bench diff $(A) $(B)

bench-baseline: build-tt-stats ## refresh benches/results/baseline.json (tt_stats build)
	@$(VPY) -m hammerhead.cli bench all --time-ms $(BENCH_TIME_MS) --tt-stats
	@latest=$$(ls -t benches/results/*.json | grep -v baseline | head -1); \
	    cp "$$latest" benches/results/baseline.json; \
	    echo "baseline updated from $$latest"

flamegraph: ## [Phase 12] capture bench_search flamegraph SVG (requires perf + cargo-flamegraph)
	@./scripts/flamegraph.sh

pgo: ## [Phase 14] profile-guided optimization build (requires llvm-tools-preview)
	@./scripts/pgo_build.sh

# ──────────────────────────────────────────────────────────────────────────────
# Phase 11 — promotion harness. See specs/SPEC_ROADMAP.md § Phase 11.
# Reads .bestref; builds a worktree at that SHA via scripts/setup_worktree.sh.
# ──────────────────────────────────────────────────────────────────────────────

vs: ## [Phase 11] current vs best, N_GAMES games — does not advance .bestref
	@$(VPY) -m hammerhead.cli promote --dry-run \
	    --n $(N_GAMES) --time-ms $(TIME_MS) --test $(TEST) \
	    --workers $(N_WORKERS)

promote: ## [Phase 11] advance .bestref to HEAD if match verdict is PROMOTE
	@$(VPY) -m hammerhead.cli promote \
	    --n $(N_GAMES) --time-ms $(TIME_MS) --test $(TEST) \
	    --workers $(N_WORKERS)

# ──────────────────────────────────────────────────────────────────────────────
# Phase 28B-1 — coordinate-descent sweep driver.
# See specs/SPEC_BENCHMARKS.md + plan § C "Commit B-1.2".
# STAGE = A|B|C (endpoint pre-screen / Stage-1 screen / Stage-2 validation).
# PARAM = e.g. open_4 or window_k_scores[5].
# GRID  = comma-separated integer cell values.
# GAMES = games per cell (default: 200 for A/B, 400 for C).
# TIME_MS_TUNE / WORKERS_TUNE / OUT override the sweep driver defaults.
# ──────────────────────────────────────────────────────────────────────────────

TIME_MS_TUNE  ?= 500
WORKERS_TUNE  ?= 10
OUT           ?= benches/results/tune

tune: ## [Phase 28B-1] run one stage of one candidate sweep (STAGE, PARAM, GRID, [GAMES])
	@$(VPY) -m hammerhead.cli bench tune-sweep \
	    --stage $(STAGE) --param $(PARAM) --grid $(GRID) \
	    $(if $(GAMES),--games $(GAMES),) \
	    --time-ms $(TIME_MS_TUNE) --workers $(WORKERS_TUNE) \
	    --out $(OUT)

tune-smoke: ## [Phase 28B-1] 5g/cell wiring-verification sweep (STAGE, PARAM, GRID)
	@$(VPY) -m hammerhead.cli bench tune-sweep --smoke \
	    --stage $(STAGE) --param $(PARAM) --grid $(GRID) \
	    --time-ms $(TIME_MS_TUNE) --workers $(WORKERS_TUNE) \
	    --out $(OUT)
