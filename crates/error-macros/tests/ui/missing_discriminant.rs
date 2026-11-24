// Verifies that an implicit (missing) discriminant that collides with an
// existing explicit one is rejected at compile time.
//
// In `declare_contract_error!` enums, all variants must carry explicit
// discriminants so their on-chain codes are never silently changed.
// This test demonstrates the failure mode: when an explicit discriminant
// is missing and the compiler-assigned implicit value collides with an
// earlier variant, the build fails.
//
// Expected: does not compile — discriminant value `2` already exists

use error_macros::declare_contract_error;

declare_contract_error! {
    /// Enum where an implicit discriminant collides with an explicit one.
    pub enum MissingDiscriminantError {
        First = 1,
        // Second gets implicit value 2 (First + 1).
        Second,
        // Explicit 2 collides with Second's implicit 2.
        AlsoTwo = 2,
    }
}

fn main() {}
