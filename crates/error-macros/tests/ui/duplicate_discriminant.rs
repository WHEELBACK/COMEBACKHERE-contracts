// Verifies that `declare_contract_error!` rejects duplicate discriminant values.
//
// The settlement-workflow crate previously had a duplicate `= 38` discriminant
// that went undetected until this test category was added (#784). The Rust
// compiler, via `#[repr(u32)]`, diagnoses duplicate values as a hard error.
//
// Expected: does not compile — "discriminant value `1` already exists"

use error_macros::declare_contract_error;

declare_contract_error! {
    /// Test enum with a duplicate discriminant — must not compile.
    pub enum DuplicateDiscriminantError {
        First = 1,
        AlsoFirst = 1,
        Second = 2,
    }
}

fn main() {}
