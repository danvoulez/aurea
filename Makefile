.PHONY: all build test fmt clippy check docs docs-strict smoke smoke-all \
        smoke-idem smoke-repair smoke-dual-control smoke-policy smoke-verify \
        smoke-metrics smoke-export artifacts-dirs release-check

# ── Defaults ────────────────────────────────────────────────────────────────
AUREA_URL ?= http://localhost:8080
ARTIFACTS := artifacts/smoke artifacts/metrics artifacts/reports

# ── Core Rust targets ────────────────────────────────────────────────────────
all: build

build:
	cargo build --workspace

build-release:
	cargo build --workspace --release

fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all --check

clippy:
	cargo clippy --workspace --all-targets -- -D warnings

test:
	cargo test --workspace

nextest:
	cargo nextest run --workspace

check: fmt-check clippy nextest

# ── Documentation ────────────────────────────────────────────────────────────
docs:
	mkdocs build

docs-strict:
	mkdocs build --strict

docs-serve:
	mkdocs serve

# ── i18n ─────────────────────────────────────────────────────────────────────
i18n-check:
	python3 .github/scripts/check_i18n_keys.py

# ── Smoke tests ──────────────────────────────────────────────────────────────
artifacts-dirs:
	@mkdir -p $(ARTIFACTS)

smoke-idem: artifacts-dirs
	@bash tools/smoke_idem.sh $(AUREA_URL)

smoke-repair: artifacts-dirs
	@bash tools/smoke_repair.sh $(AUREA_URL)

smoke-dual-control: artifacts-dirs
	@bash tools/smoke_dual_control.sh $(AUREA_URL)

smoke-policy: artifacts-dirs
	@bash tools/smoke_policy.sh $(AUREA_URL)

smoke-verify: artifacts-dirs
	@bash tools/smoke_verify.sh $(AUREA_URL)

smoke-metrics: artifacts-dirs
	@bash tools/smoke_metrics.sh $(AUREA_URL)

smoke-export: artifacts-dirs
	@bash tools/smoke_export.sh $(AUREA_URL)

smoke-all: artifacts-dirs
	@bash tools/smoke_all.sh $(AUREA_URL)

# ── Release gate ─────────────────────────────────────────────────────────────
release-check: check docs-strict i18n-check
	@echo "All CI gates passed."

# ── Run the server (dev mode) ─────────────────────────────────────────────────
serve:
	cargo run --bin aurea -- serve

serve-release:
	cargo run --release --bin aurea -- serve
