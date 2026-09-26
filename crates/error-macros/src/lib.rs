#![no_std]

//! `declare_contract_error!` — the shared macro behind every
//! `#[contracterror]` enum in this workspace.
//!
//! # Why
//!
//! The COMEBACKHERE protocol has four `#[contracterror]` enums, all of which had
//! to repeat the same three-attribute block by hand:
//!
//! ```ignore
//! #[contracterror]
//! #[derive(Copy, Clone, Debug, Eq, PartialEq)]
//! #[repr(u32)]
//! pub enum TreasuryError { /* ... */ }
//! ```
//!
//! Two problems with hand-repeating that block:
//!
//! - **It drifts.** The derive list had already diverged across crates —
//!   `InvoiceError` and `TreasuryError` derived `Eq`, `ComplianceError` and
//!   `ContractError` did not. Nothing failed when that happened, so the drift
//!   was invisible.
//! - **It is load-bearing boilerplate.** `#[repr(u32)]` in particular is what
//!   makes the discriminants ABI-visible, and forgetting it would silently
//!   change every code a contract returns.
//!
//! The macro removes the hand-written block and pins the convention in one
//! place, so the attributes can no longer drift apart.
//!
//! # Usage
//!
//! Add the crate as a dependency and invoke the macro. Note that the
//! `use soroban_sdk::contracterror;` import that every error crate used to
//! carry is **no longer needed** — the macro emits a fully-qualified
//! `#[$crate::soroban_sdk::contracterror]`, so it resolves against this crate's
//! own `soroban_sdk` re-export regardless of what the calling crate imports.
//!
//! ```ignore
//! use error_macros::declare_contract_error;
//!
//! declare_contract_error! {
//!     /// Error codes for the thing contract.
//!     ///
//!     /// Variants are append-only and must never be renumbered; discriminants
//!     /// are part of the on-chain ABI. See `scripts/check-enum-ordering.sh`.
//!     pub enum ThingError {
//!         AlreadyInitialized = 1,
//!         SomethingElse = 2,
//!     }
//! }
//! ```
//!
//! expands to exactly:
//!
//! ```ignore
//! /// Error codes for the thing contract.
//! ///
//! /// Variants are append-only and must never be renumbered; discriminants
//! /// are part of the on-chain ABI. See `scripts/check-enum-ordering.sh`.
//! #[contracterror]
//! #[derive(Copy, Clone, Debug, PartialEq, Eq)]
//! #[repr(u32)]
//! pub enum ThingError {
//!     AlreadyInitialized = 1,
//!     SomethingElse = 2,
//! }
//! ```
//!
//! # What it generates
//!
//! | Part | Value | Why it is fixed |
//! |---|---|---|
//! | `#[contracterror]` | `soroban_sdk::contracterror` | Marks the enum as a Soroban contract error, giving it the `TryFrom`/scerror conversion the host uses |
//! | `#[derive(...)]` | `Copy, Clone, Debug, PartialEq, Eq` | Uniform across all error enums. `Eq` is added to every enum (rather than left off some) because adding a trait impl cannot break an existing bound, whereas removing one can |
//! | `#[repr(u32)]` | always | Without it the discriminants would not be the ABI-visible `u32` codes callers match on |
//!
//! The variant list — names, order, doc comments and **explicit discriminants** —
//! is written out by hand in the invocation and passed through untouched. The
//! macro never renumbers, reorders, or infers a code, so adopting it cannot
//! change a single numeric value.
//!
//! # Invariant: the macro must stay transparent
//!
//! Because these discriminants are on-chain ABI, this crate is deliberately
//! dumb. If you find yourself wanting the macro to do more — auto-numbering
//! variants, generating `Display`, inserting `impl` blocks, deriving per-enum
//! — do not add it. Every one of those is a place where a future edit to the
//! macro could silently renumber an existing code across four contracts at
//! once. Declare a hand-written `#[contracterror]` enum instead; the macro is an
//! ergonomic convenience, not a framework.
//!
//! The escape hatch is always available: nothing stops a crate from writing
//! `#[contracterror] #[derive(...)] #[repr(u32)] pub enum Foo { .. }` by hand,
//! and both `scripts/check-enum-ordering.sh` and
//! `scripts/check-enum-doc-comments.sh` validate hand-written and macro-generated
//! enums alike.
//!
//! # Adding a new error crate
//!
//! 1. `crates/<name>-errors/Cargo.toml`, mirroring the other error crates:
//!    `#![no_std]`, `soroban-sdk.workspace = true`, and
//!    `error-macros = { package = "comebackhere-error-macros", path = "../error-macros" }`.
//! 2. Pick the crate's numeric range so it does not collide with another
//!    contract's — see `ARCHITECTURE.md`'s "Error-Code Ranges per Contract" and
//!    the per-crate READMEs under `crates/`.
//! 3. Start the enum at `1` and document in its rustdoc that variants are
//!    append-only.
//! 4. Add a README with a code/name/meaning table, matching
//!    `crates/invoice-errors/README.md`.
//! 5. Do **not** add the crate to `crates/protocol-errors` without review: that
//!    changes `ProtocolError`'s variant set, which is pinned by a
//!    wildcard-free `match` in `crates/protocol-errors/tests/`.

/// Re-exported so `declare_contract_error!` can emit a fully-qualified
/// `#[contracterror]` attribute and therefore be usable without every caller
/// repeating `use soroban_sdk::contracterror;`.
pub use soroban_sdk;

/// Declares a Soroban `#[contracterror]` enum with a fixed, non-drifting
/// attribute block. See the [crate-level docs](crate) for the full contract:
/// what it expands to, why each part is fixed, and the invariants a change to
/// this macro must preserve.
///
/// The variant list is passed through verbatim, including explicit
/// discriminants — this macro never renumbers a code.
///
/// # Example
///
/// ```ignore
/// declare_contract_error! {
///     /// Error codes for the widget contract. Append-only; see
///     /// `scripts/check-enum-ordering.sh`.
///     pub enum WidgetError {
///         Unauthorized = 1,
///         NotFound = 2,
///     }
/// }
/// ```
#[macro_export]
macro_rules! declare_contract_error {
    (
        $(#[$attr:meta])*
        pub enum $name:ident {
            $($body:tt)*
        }
    ) => {
        $(#[$attr])*
        #[$crate::soroban_sdk::contracterror]
        #[derive(Copy, Clone, Debug, PartialEq, Eq)]
        #[repr(u32)]
        pub enum $name {
            $($body)*
        }
    };
}

#[cfg(test)]
extern crate std;
