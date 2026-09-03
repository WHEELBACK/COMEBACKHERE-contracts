.PHONY: build test test-integration fmt lint check audit check-enum-ordering mutants-treasury precommit

build:
	cargo build

test:
	cargo test

test-integration:
	cargo test -p tests

fmt:
	cargo fmt --all

lint:
	cargo clippy -- -D warnings

audit:
	cargo audit

# #74: Validate that all #[repr(u32)] error enums are append-only
check-enum-ordering:
	./scripts/check-enum-ordering.sh

mutants-treasury:
	cargo mutants --package comebackhere-treasury --timeout 60 --test-threads 1 --in-place --no-shuffle --list-test-cases --exclude "contracts/treasury/tests/" || true

check: fmt lint check-enum-ordering test
	@echo "✓ All checks passed"

# Literal mirror of the Pre-commit CI job's command sequence (see .github/workflows/pre-commit.yml
# and .pre-commit-config.yaml) — run this before pushing for an authoritative local answer to
# "will Pre-commit pass".
precommit:
	cargo fmt --all -- --check
	cargo clippy -- -D warnings
	./scripts/check-enum-ordering.sh
	./scripts/check-enum-doc-comments.sh
	@echo "✓ Pre-commit checks passed"
