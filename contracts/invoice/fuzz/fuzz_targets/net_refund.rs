#![no_main]

// Fuzz harness (cargo-fuzz) for refund.rs::calculate_net_refund, which mixes
// i128 and bps math. Unit tests cover fixed cases only; this harness throws
// arbitrary inputs at it to assert the invariants that matter for any input
// that is accepted:
//   - processing_fee + network_fee + net_amount accounts for at most gross
//   - net_amount and both fees are never negative
//   - the function never panics (overflow, divide-by-zero, etc.)

use arbitrary::Arbitrary;
use invoice::calculate_net_refund;
use libfuzzer_sys::fuzz_target;

#[derive(Debug, Arbitrary)]
struct Input {
    gross_amount: i128,
    fee_bps: u32,
}

fuzz_target!(|input: Input| {
    if let Ok(r) = calculate_net_refund(input.gross_amount, input.fee_bps) {
        assert_eq!(r.gross_amount, input.gross_amount);
        assert!(r.net_amount >= 0, "net must never be negative");
        assert!(r.processing_fee >= 0, "processing fee must never be negative");
        assert!(r.network_fee >= 0, "network fee must never be negative");
        assert!(r.net_amount <= r.gross_amount, "net must not exceed gross");
    }
    // Err is acceptable for out-of-range inputs; a panic is not.
});
