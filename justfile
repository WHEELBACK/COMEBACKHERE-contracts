# COMEBACKHERE-contracts task runner

# Compile all contracts
build:
    cargo build

# Run all tests
test:
    cargo test

# Format all code
fmt:
    cargo fmt --all

# Run clippy lints
lint:
    cargo clippy -- -D warnings

# Check dependencies for vulnerabilities and license issues
deny:
    cargo deny check

# Audit dependencies for security vulnerabilities
audit:
    cargo audit

# Expand macros for a specific contract
expand contract:
    cargo expand -p {{contract}}

# Run format and lint checks (for CI)
check: fmt lint test deny
    @echo "✓ All checks passed"

# Default target
default: check
