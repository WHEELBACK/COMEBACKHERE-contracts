//! Proves `declare_contract_error!` expands to a real `#[contracterror]` enum
//! with the exact attributes it documents — and, critically, that it does so
//! **transparently**.
//!
//! The macro exists only to remove a repeated three-attribute block, so the
//! failure mode that would actually hurt is subtle: the macro quietly changing
//! a discriminant, dropping `#[repr(u32)]`, or emitting a plain enum that
//! happens to compile but is not a Soroban `contracterror` at all. Any of those
//! would be invisible at the call site, because the four real error enums are
//! consumed by contracts that never inspect their ABI representation directly.
//!
//! These tests assert the generated surface from the outside: explicit
//! discriminants survive verbatim, the enum converts to and from the
//! `soroban_sdk::Error` the host puts on the wire, the generated contract-spec
//! entry carries the same codes, and every documented derive is present
//! (checked by calling the trait methods, so a missing derive is a compile
//! error).
//!
//! Run with: cargo test --package comebackhere-error-macros

use error_macros::declare_contract_error;
use soroban_sdk::{
    xdr::{Limits, ReadXdr, ScErrorType, ScSpecEntry, ScSpecUdtErrorEnumV0},
    Error,
};

declare_contract_error! {
    /// Doc comment on the enum itself — `scripts/check-enum-doc-comments.sh`
    /// requires one, and the macro must forward it to the generated enum.
    pub enum SampleError {
        First = 1,
        Second = 2,
        Third = 3,
    }
}

// A second enum, to show the macro is not specialised to one shape and that
// variant-level doc comments are preserved. Note the descriptive comment is a
// plain `//` rather than `///`: rustdoc does not document macro invocations, so
// `///` here would be a `unused_doc_comments` warning rather than documentation.
declare_contract_error! {
    /// Single-variant enum with a documented variant.
    pub enum DocumentedVariantError {
        /// This comment must survive the expansion.
        Only = 1,
    }
}

#[test]
fn explicit_discriminants_are_preserved_verbatim() {
    assert_eq!(SampleError::First as u32, 1);
    assert_eq!(SampleError::Second as u32, 2);
    assert_eq!(SampleError::Third as u32, 3);
    assert_eq!(DocumentedVariantError::Only as u32, 1);
}

#[test]
fn generated_enum_is_a_soroban_contract_error() {
    // `#[contracterror]` is what supplies these conversions. A plain
    // `#[derive(...)] #[repr(u32)]` enum would fail to compile here, which is
    // exactly the regression this assertion exists to catch.
    for (code, expected) in [
        (SampleError::First, 1u32),
        (SampleError::Second, 2),
        (SampleError::Third, 3),
    ] {
        let err = Error::from(code);
        assert!(
            err.is_type(ScErrorType::Contract),
            "error must be reported as a contract error"
        );
        assert_eq!(
            err.get_code(),
            expected,
            "wire code must match discriminant"
        );
        assert_eq!(SampleError::try_from(err).unwrap(), code);
    }

    let err = Error::from(DocumentedVariantError::Only);
    assert_eq!(err.get_code(), 1);
    assert_eq!(
        DocumentedVariantError::try_from(err).unwrap(),
        DocumentedVariantError::Only
    );
}

#[test]
fn generated_contract_spec_entry_matches_discriminants() {
    // The `contractspecv0` section is what a deployed contract actually
    // publishes, so decoding it proves the codes reach the ABI rather than just
    // the Rust type.
    let entry = ScSpecEntry::from_xdr(SampleError::spec_xdr(), Limits::none())
        .expect("generated spec must decode as XDR");
    let ScSpecEntry::UdtErrorEnumV0(ScSpecUdtErrorEnumV0 { name, cases, .. }) = entry else {
        panic!("expected a UdtErrorEnumV0 spec entry");
    };
    assert_eq!(name.to_string(), "SampleError");

    let published: Vec<(String, u32)> = cases
        .iter()
        .map(|case| (case.name.to_string(), case.value))
        .collect();
    assert_eq!(
        published,
        vec![
            ("First".to_string(), 1),
            ("Second".to_string(), 2),
            ("Third".to_string(), 3),
        ]
    );
}

#[test]
fn macro_derives_every_documented_trait() {
    // Each assertion goes through a generic bound so that dropping a derive
    // from the macro becomes a compile error here, rather than a silently
    // narrower type in every crate that uses it.
    fn assert_copy<T: Copy>(value: T) -> T {
        value
    }
    fn assert_clone<T: Clone>(value: &T) -> T {
        value.clone()
    }
    fn assert_eq_trait<T: Eq + PartialEq + core::fmt::Debug>(a: &T, b: &T) {
        assert_eq!(a, b);
    }

    let original = assert_copy(SampleError::First);
    let copied = assert_copy(original);
    let cloned = assert_clone(&original);
    assert_eq!(original, copied);
    assert_eq!(original, cloned);

    // PartialEq + Eq
    assert_eq_trait(
        &SampleError::Third,
        &SampleError::try_from(Error::from(SampleError::Third)).unwrap(),
    );

    // Debug (also what assert_eq!'s failure output requires)
    assert_eq!(format!("{:?}", SampleError::First), "First");
}

#[test]
fn different_enums_have_independent_code_spaces() {
    // The macro does not renumber or offset anything: each enum starts where
    // its author put it, which is what lets separate crates own separate
    // numeric ranges without the macro knowing about them.
    assert_eq!(
        SampleError::First as u32,
        DocumentedVariantError::Only as u32
    );
    assert_ne!(SampleError::First as u32, SampleError::Second as u32);
}
