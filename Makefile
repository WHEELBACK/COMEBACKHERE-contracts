.PHONY: build test fmt lint check audit

build:
	cargo build

test:
	cargo test

fmt:
	cargo fmt --all

lint:
	cargo clippy -- -D warnings

audit:
	cargo audit

check: fmt lint test
	@echo "✓ All checks passed"
