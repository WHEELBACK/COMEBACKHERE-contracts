.PHONY: build test fmt lint check audit check-enum-ordering

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

# #74: Validate that all #[repr(u32)] error enums are append-only
check-enum-ordering:
	./scripts/check-enum-ordering.sh

check: fmt lint check-enum-ordering test
	@echo "✓ All checks passed"
