.DEFAULT_GOAL := help
.PHONY: help build clean rebuild test lint fmt check vs promote install

ENGINE    := hexo-engine
PY        := hexo

# Phase 10 (promotion harness) defaults — override on the command line:
#   make vs N_GAMES=500 TIME_MS=2000 TEST=wilson
N_GAMES   ?= 200
TIME_MS   ?= 1000
TEST      ?= sprt
ELO_LOW   ?= 0
ELO_HIGH  ?= 5

help: ## show available targets
	@echo "HeXO bot — Makefile targets:"
	@grep -E '^[a-zA-Z_-]+:.*?## ' $(MAKEFILE_LIST) \
	  | awk -F':.*?## ' '{printf "  \033[36m%-10s\033[0m %s\n", $$1, $$2}'

build: ## maturin develop --release + pip install -e hexo
	cd $(ENGINE) && maturin develop --release
	pip install -e $(PY)

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

test: ## cargo test --release + pytest
	cd $(ENGINE) && cargo test --release
	cd $(PY) && pytest

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
# Phase 10 — promotion harness. Stubbed until baseline (Phase 9) is complete.
# See specs/SPEC_ROADMAP.md § Phase 10 for the harness specification.
# ──────────────────────────────────────────────────────────────────────────────

vs: ## [Phase 10] current vs best, N_GAMES games (override N_GAMES, TIME_MS, TEST, ELO_LOW, ELO_HIGH)
	@echo "vs harness: Phase 10 (post-baseline)."
	@echo "  N_GAMES=$(N_GAMES) TIME_MS=$(TIME_MS) TEST=$(TEST) ELO_LOW=$(ELO_LOW) ELO_HIGH=$(ELO_HIGH)"
	@echo "  See specs/SPEC_ROADMAP.md § Phase 10."
	@exit 1

promote: ## [Phase 10] advance .bestref to HEAD if vs passes threshold
	@echo "promote: Phase 10 (post-baseline)."
	@echo "  See specs/SPEC_ROADMAP.md § Phase 10."
	@exit 1
