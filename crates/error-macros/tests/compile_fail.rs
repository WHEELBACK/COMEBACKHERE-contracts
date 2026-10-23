//! Compile-fail UI tests for `declare_contract_error!` (#784).
//!
//! Verifies that the macro rejects invalid enum definitions at compile time
//! with readable diagnostics, using `trybuild` to run each test file as a
//! separate compilation and compare the compiler output against a stored
//! `.stderr` snapshot.
//!
//! Run with: `cargo test -p comebackhere-error-macros`
//!
//! On first run (or after intentional changes to error output), trybuild
//! writes/updates the `.stderr` files under `tests/ui/`. Commit those files
//! alongside the `.rs` sources.

#[test]
fn compile_fail_ui_tests() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/*.rs");
}
