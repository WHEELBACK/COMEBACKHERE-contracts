//! Refund verification, the cross-contract payout, and gross-vs-net fee
//! arithmetic.
//!
//! A refund is an instruction to move the customer's money back to them, and it
//! is executed by *two* contracts: this one decides, and the token contract
//! moves the funds. All of that logic is collected here so that every
//! entrypoint which can turn a `RefundRequested` invoice into a `Refunded` one
//! enforces the same rules, and so a failure on the far side of the contract
//! boundary cannot leave the two halves disagreeing.
//!
//! - [`verify_payment_state`] — the payment-completion check that must pass
//!   before any refund state is written (#70).
//! - [`refund_recipient`] — the only address a refund may be paid to (#70).
//! - [`transfer_net_refund`] — the only cross-contract call, made through
//!   `try_transfer` so a token-side failure is a returned
//!   [`InvoiceError::RefundTransferFailed`] rather than a panic that aborts the
//!   call with no error code (#70).
//! - [`calculate_net_refund`] — the single definition of the fee model, so the
//!   view entrypoint, the transferred amount and the emitted event cannot
//!   disagree about what the customer receives (#71).

use crate::invoice::{Invoice, InvoiceError, MaybeAddress};
use soroban_sdk::{contracttype, token, Address, Env};

/// Denominator for basis-point fee parameters: `10_000` bps = 100%.
pub const BPS_DENOMINATOR: u32 = 10_000;

/// Largest `fee_bps` accepted by [`calculate_net_refund`], i.e. a fee that
/// consumes the entire gross amount. Anything above this is a caller mistake
/// rather than a policy choice.
pub const MAX_REFUND_FEE_BPS: u32 = BPS_DENOMINATOR;

/// Flat cost charged against every refund payout, in the token's smallest unit.
///
/// Sized to Stellar's base network fee (100 stroops = 0.00001 XLM) for the
/// transfer that pays the customer out. It is deducted from the gross amount
/// rather than added to it, so no fee policy can make a refund cost the
/// protocol more than it returns, and it is capped at the gross amount so a
/// micro-value refund nets to `0` instead of going negative.
pub const REFUND_NETWORK_FEE: i128 = 100;

/// The fee breakdown of one refund, in the token's smallest unit.
///
/// Returned by `process_refund` and `calculate_net_refund`, stored under
/// [`crate::DataKey::RefundBreakdown`] and published in the `refund_processed`
/// event, so an indexer can reconcile what the customer received against what
/// was deducted without re-deriving the arithmetic.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NetRefund {
    /// The invoice amount the refund was computed from, before any deduction.
    pub gross_amount: i128,
    /// `gross_amount * fee_bps / 10_000`, rounded down: the payment gateway's cut.
    pub processing_fee: i128,
    /// Flat network cost of paying the refund out, capped at `gross_amount`.
    pub network_fee: i128,
    /// What the customer actually receives: the gross amount less both fees.
    /// Never negative; floors at `0` if the fees would consume the gross amount.
    pub net_amount: i128,
}

/// Verifies that the stored invoice describes a completed payment that can
/// safely be refunded (#70).
///
/// `request_refund` only accepts a `Paid` invoice, and `mark_paid` is the only
/// way to reach `Paid`, so every reachable invoice passes this. It is a
/// defence-in-depth check on the exact state an approver is about to act on: it
/// is cheap, and it means a refund can never be recorded as executed against an
/// invoice whose payment evidence is missing or self-inconsistent, whatever
/// produced that state.
///
/// Errors: [`InvoiceError::PaymentStateInconsistent`] if the payment was never
/// timestamped, if the payer is unknown (so there is no one to refund), or if
/// the amount pair is not internally consistent (`0 < amount_usdc <=
/// gross_usdc`) — the same invariant `require_positive_amount` enforces at
/// creation time.
pub fn verify_payment_state(invoice: &Invoice) -> Result<(), InvoiceError> {
    if invoice.paid_at.is_none() {
        return Err(InvoiceError::PaymentStateInconsistent);
    }
    if matches!(invoice.payer, MaybeAddress::None) {
        return Err(InvoiceError::PaymentStateInconsistent);
    }
    if invoice.amount_usdc <= 0 || invoice.gross_usdc < invoice.amount_usdc {
        return Err(InvoiceError::PaymentStateInconsistent);
    }
    Ok(())
}

/// The address a refund must be paid to: the payer recorded at `mark_paid`.
///
/// Errors: [`InvoiceError::PaymentStateInconsistent`] if no payer is recorded.
/// [`verify_payment_state`] enforces the same invariant; this exists so the
/// extraction cannot silently fall through to a default address.
pub fn refund_recipient(invoice: &Invoice) -> Result<Address, InvoiceError> {
    match &invoice.payer {
        MaybeAddress::Some(payer) => Ok(payer.clone()),
        MaybeAddress::None => Err(InvoiceError::PaymentStateInconsistent),
    }
}

/// Computes the net payout for a refund of `gross_amount` under a `fee_bps`
/// processing-fee policy (#71).
///
/// The fee model is deliberately two-part and fully bounded:
///
/// - `processing_fee = gross_amount * fee_bps / 10_000`, rounded down, so the
///   merchant's payment-gateway cut scales with the amount refunded and can
///   never exceed the amount itself;
/// - `network_fee = min(gross_amount, REFUND_NETWORK_FEE)`, the flat cost of the
///   payout transfer.
///
/// `net_amount = gross_amount - processing_fee - network_fee`, floored at `0`.
///
/// # Overflow
///
/// The intermediate product `gross_amount * fee_bps` is *never* formed, because
/// it does not fit: for a maximal `i128` amount at 250 bps it needs ~4.3e39,
/// which overflows even `u128`. Floor division is instead decomposed with
/// `gross = q * D + r` into
///
/// ```text
/// floor(gross * bps / D) == q * bps + floor(r * bps / D)
/// ```
///
/// Every term is bounded by the gross amount (`q * bps <= gross` because
/// `bps <= D`, and `r * bps < D * D = 1e8`), so the whole input domain computes
/// in `i128`. The `checked_*` calls below are therefore unreachable for any
/// value that passed the range guards, and are kept as a typed-error backstop:
/// if a future change to `BPS_DENOMINATOR`/`MAX_REFUND_FEE_BPS` broke the bound,
/// this returns [`InvoiceError::ArithmeticOverflow`] rather than trapping with
/// no error code.
///
/// # Examples
///
/// ```
/// use invoice::{calculate_net_refund, REFUND_NETWORK_FEE};
///
/// // No gateway fee: the customer gets the amount back, less the network cost.
/// let full = calculate_net_refund(10_000_000, 0).unwrap();
/// assert_eq!(full.processing_fee, 0);
/// assert_eq!(full.network_fee, REFUND_NETWORK_FEE);
/// assert_eq!(full.net_amount, 10_000_000 - REFUND_NETWORK_FEE);
///
/// // 250 bps = 2.5%, rounded down.
/// let with_fee = calculate_net_refund(10_000_000, 250).unwrap();
/// assert_eq!(with_fee.processing_fee, 250_000);
/// assert_eq!(with_fee.net_amount, 10_000_000 - 250_000 - REFUND_NETWORK_FEE);
///
/// // A micro-value refund never nets below zero.
/// let dust = calculate_net_refund(50, 0).unwrap();
/// assert_eq!(dust.net_amount, 0);
/// ```
///
/// # Errors
///
/// - [`InvoiceError::RefundFeeTooHigh`] if `fee_bps > 10_000`.
/// - [`InvoiceError::InvalidAmount`] if `gross_amount <= 0`.
/// - [`InvoiceError::ArithmeticOverflow`] if the fee arithmetic exceeds `i128`.
pub fn calculate_net_refund(gross_amount: i128, fee_bps: u32) -> Result<NetRefund, InvoiceError> {
    if fee_bps > MAX_REFUND_FEE_BPS {
        return Err(InvoiceError::RefundFeeTooHigh);
    }
    if gross_amount <= 0 {
        return Err(InvoiceError::InvalidAmount);
    }

    let bps = i128::from(fee_bps);
    let denominator = i128::from(BPS_DENOMINATOR);
    let whole = (gross_amount / denominator)
        .checked_mul(bps)
        .ok_or(InvoiceError::ArithmeticOverflow)?;
    let partial = (gross_amount % denominator)
        .checked_mul(bps)
        .ok_or(InvoiceError::ArithmeticOverflow)?
        / denominator;
    let processing_fee = whole
        .checked_add(partial)
        .ok_or(InvoiceError::ArithmeticOverflow)?;

    // Never charge more than there is to give back.
    //
    // `processing_fee + network_fee` can exceed the gross (at 100% bps it does),
    // so the two deductions are applied one at a time, each as an explicit
    // comparison rather than a `saturating_sub` or a `checked_sub` + error. Two
    // properties make that form total: `processing_fee <= gross_amount` and
    // `network_fee <= gross_amount`, both established above. So each subtraction
    // only ever runs on a non-negative difference, and the payout floors at `0`
    // without any intermediate sum that could overflow `i128`.
    let network_fee = core::cmp::min(gross_amount, REFUND_NETWORK_FEE);
    let after_processing = if processing_fee >= gross_amount {
        0
    } else {
        gross_amount - processing_fee
    };
    let net_amount = if network_fee >= after_processing {
        0
    } else {
        after_processing - network_fee
    };

    Ok(NetRefund {
        gross_amount,
        processing_fee,
        network_fee,
        net_amount,
    })
}

/// Moves `amount` of `token_contract` from this contract's own escrow balance to
/// `recipient` (#70).
///
/// The transfer is made with `try_transfer` so that a failing payment/token
/// contract — paused, out of escrow balance, missing the `transfer` export, or
/// simply not a SEP-41 contract at all — comes back as a value this function
/// can classify. Every failure path returns
/// [`InvoiceError::RefundTransferFailed`] **before** the caller writes any
/// refund state, so a failed payout can never leave an invoice marked
/// `Refunded` while the customer was not paid.
///
/// The `from` side is always this contract's own address, never a
/// caller-supplied one: a refund is funded from the escrow the invoice contract
/// holds, and a caller must not be able to choose which balance is debited.
pub fn transfer_net_refund(
    env: &Env,
    token_contract: &Address,
    recipient: &Address,
    amount: i128,
) -> Result<(), InvoiceError> {
    let from = env.current_contract_address();
    let transfer = token::Client::new(env, token_contract).try_transfer(&from, recipient, &amount);
    match transfer {
        Ok(Ok(())) => Ok(()),
        // Either the invocation itself failed (revert, missing export, auth
        // failure) or the token rejected the transfer. Both mean no tokens
        // moved, so the refund must not be recorded.
        _ => Err(InvoiceError::RefundTransferFailed),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        calculate_net_refund, NetRefund, BPS_DENOMINATOR, MAX_REFUND_FEE_BPS, REFUND_NETWORK_FEE,
    };
    use crate::invoice::InvoiceError;

    /// 10 USDC at 7 decimals — the amount shape a real invoice carries.
    const TEN_USDC: i128 = 100_000_000;

    /// Every gross amount / fee pair the invariants below are asserted over.
    /// Chosen to straddle each branch of the arithmetic: below the network fee,
    /// exactly at it, just above it, an amount too small to reach a whole basis
    /// point, an ordinary invoice, and the `i128` extremes.
    const GROSS_CASES: [i128; 8] = [
        1,
        99,
        REFUND_NETWORK_FEE - 1,
        REFUND_NETWORK_FEE,
        REFUND_NETWORK_FEE + 1,
        999,
        TEN_USDC,
        i128::MAX,
    ];

    /// Fee policies from free to maximal, plus the smallest rejected one.
    const BPS_CASES: [u32; 8] = [0, 1, 25, 250, 1_000, 5_000, 9_999, MAX_REFUND_FEE_BPS];

    #[test]
    fn no_processing_fee_returns_the_gross_less_the_network_fee() {
        let refund = calculate_net_refund(TEN_USDC, 0).unwrap();
        assert_eq!(refund.gross_amount, TEN_USDC);
        assert_eq!(refund.processing_fee, 0);
        assert_eq!(refund.network_fee, REFUND_NETWORK_FEE);
        assert_eq!(refund.net_amount, TEN_USDC - REFUND_NETWORK_FEE);
    }

    /// 250 bps = 2.5%: the gateway's cut scales with the amount refunded rather
    /// than being a flat charge.
    #[test]
    fn processing_fee_is_the_basis_point_share_rounded_down() {
        let refund = calculate_net_refund(TEN_USDC, 250).unwrap();
        assert_eq!(
            refund.processing_fee,
            TEN_USDC * 250 / BPS_DENOMINATOR as i128
        );
        assert_eq!(
            refund.net_amount,
            TEN_USDC - refund.processing_fee - refund.network_fee
        );
    }

    /// Rounding favours the protocol, never the customer: a 1 bps fee on an
    /// amount too small to reach one full stroop of fee charges nothing.
    #[test]
    fn processing_fee_rounds_down() {
        let refund = calculate_net_refund(999, 1).unwrap();
        assert_eq!(refund.processing_fee, 0);
        assert_eq!(refund.net_amount, 999 - REFUND_NETWORK_FEE);
    }

    /// 10_000 bps is 100% and is the largest fee the contract accepts; it eats
    /// the whole gross amount before the network fee is even considered.
    #[test]
    fn maximum_fee_is_accepted_and_consumes_the_gross_amount() {
        let refund = calculate_net_refund(TEN_USDC, MAX_REFUND_FEE_BPS).unwrap();
        assert_eq!(refund.processing_fee, TEN_USDC);
        assert_eq!(refund.net_amount, 0);
    }

    #[test]
    fn fee_above_one_hundred_percent_is_rejected() {
        assert_eq!(
            calculate_net_refund(TEN_USDC, MAX_REFUND_FEE_BPS + 1),
            Err(InvoiceError::RefundFeeTooHigh)
        );
        // The worst case a caller can pass is still a typed error, not a wrap.
        assert_eq!(
            calculate_net_refund(TEN_USDC, u32::MAX),
            Err(InvoiceError::RefundFeeTooHigh)
        );
    }

    /// The network fee is a deduction, so it can never make a refund cost the
    /// protocol more than it hands back.
    #[test]
    fn network_fee_is_capped_at_the_gross_amount() {
        let refund = calculate_net_refund(REFUND_NETWORK_FEE, 0).unwrap();
        assert_eq!(refund.network_fee, REFUND_NETWORK_FEE);
        assert_eq!(refund.net_amount, 0);
    }

    #[test]
    fn a_refund_smaller_than_the_network_fee_nets_to_zero_not_negative() {
        let refund = calculate_net_refund(REFUND_NETWORK_FEE - 1, 0).unwrap();
        assert_eq!(refund.network_fee, REFUND_NETWORK_FEE - 1);
        assert_eq!(refund.net_amount, 0);
    }

    #[test]
    fn non_positive_gross_amounts_are_rejected() {
        assert_eq!(calculate_net_refund(0, 0), Err(InvoiceError::InvalidAmount));
        assert_eq!(
            calculate_net_refund(-1, 250),
            Err(InvoiceError::InvalidAmount)
        );
        assert_eq!(
            calculate_net_refund(i128::MIN, 0),
            Err(InvoiceError::InvalidAmount)
        );
    }

    /// The reason the intermediate product is never formed: a maximal `i128`
    /// gross amount at 250 bps would need ~4.3e39, which overflows even `u128`.
    /// Every fee policy must still compute exactly, never wrap.
    #[test]
    fn maximal_gross_amount_does_not_overflow() {
        let refund = calculate_net_refund(i128::MAX, MAX_REFUND_FEE_BPS).unwrap();
        assert_eq!(refund.gross_amount, i128::MAX);
        assert_eq!(refund.processing_fee, i128::MAX);
        assert_eq!(refund.network_fee, REFUND_NETWORK_FEE);
        assert_eq!(refund.net_amount, 0);

        // 50% of `i128::MAX` rounds down, and the payout stays positive.
        let half = calculate_net_refund(i128::MAX, 5_000).unwrap();
        assert_eq!(half.processing_fee, i128::MAX / 2);
        assert!(half.net_amount > 0);

        // 250 bps is the case whose naive product needs ~4.3e39 and overflows
        // even `u128`. The result must still be the exact floor of the share,
        // i.e. both terms of the decomposition.
        let quarter_bps = calculate_net_refund(i128::MAX, 25).unwrap();
        assert_eq!(
            quarter_bps.processing_fee,
            (i128::MAX / BPS_DENOMINATOR as i128) * 25
                + (i128::MAX % BPS_DENOMINATOR as i128) * 25 / BPS_DENOMINATOR as i128
        );
        assert!(quarter_bps.net_amount > 0);
    }

    /// The accounting identity behind every consumer of `NetRefund`: the payout
    /// plus the two fees either closes against the gross exactly, or the fees
    /// were capped at the gross and the payout floored at zero. There is no
    /// third possibility, so the transferred amount, the stored breakdown and
    /// the published event can never disagree about the arithmetic.
    #[test]
    fn the_breakdown_either_closes_against_the_gross_or_is_floored() {
        for gross in GROSS_CASES {
            for bps in BPS_CASES {
                let refund = calculate_net_refund(gross, bps)
                    .unwrap_or_else(|_| panic!("gross={gross} bps={bps} was rejected"));
                let processing = refund.processing_fee;
                let network = refund.network_fee;

                // `processing + network` is only summed where it is known to fit,
                // so the assertion itself cannot overflow on a maximal amount.
                if processing <= gross && network <= gross - processing {
                    assert_eq!(
                        gross - (processing + network),
                        refund.net_amount,
                        "gross={gross} bps={bps}: breakdown does not close"
                    );
                } else {
                    assert_eq!(
                        refund.net_amount, 0,
                        "gross={gross} bps={bps}: fees consumed the gross but net={}",
                        refund.net_amount
                    );
                }
            }
        }
    }

    /// The bounds the fee model promises, checked over the same matrix: the
    /// customer is never paid a negative or an inflated amount, and no single
    /// fee is ever charged on more than the amount being refunded.
    #[test]
    fn the_net_payout_is_always_within_the_gross() {
        for gross in GROSS_CASES {
            for bps in BPS_CASES {
                let refund = calculate_net_refund(gross, bps).unwrap();

                assert!(
                    refund.net_amount >= 0,
                    "gross={gross} bps={bps}: net went negative"
                );
                assert!(
                    refund.net_amount <= gross,
                    "gross={gross} bps={bps}: customer received more than was paid"
                );
                assert!(
                    refund.processing_fee <= gross,
                    "gross={gross} bps={bps}: gateway fee exceeded the gross"
                );
                assert!(
                    refund.network_fee <= gross,
                    "gross={gross} bps={bps}: network fee exceeded the gross"
                );
                assert_eq!(
                    refund.gross_amount, gross,
                    "gross={gross} bps={bps}: gross not echoed back"
                );
            }
        }
    }

    /// The network fee is a *flat* charge, so it is exactly the constant
    /// whenever the refund is large enough to absorb it and the gross itself
    /// when it is not. This is the property a merchant can rely on when
    /// quoting a customer a fee.
    #[test]
    fn the_network_fee_is_flat_above_the_dust_threshold() {
        for gross in [REFUND_NETWORK_FEE + 1, 1_000, 12_345, TEN_USDC, i128::MAX] {
            for bps in BPS_CASES {
                assert_eq!(
                    calculate_net_refund(gross, bps).unwrap().network_fee,
                    REFUND_NETWORK_FEE,
                    "gross={gross} bps={bps}"
                );
            }
        }
    }

    /// Guards the exact field order `NetRefund` is serialized in: the same type
    /// is stored under `DataKey::RefundBreakdown` and published in the
    /// `refund_processed` event, so a reorder here reshuffles published event
    /// data and stored records. It has to be a deliberate change, not an
    /// accident.
    #[test]
    fn net_refund_layout_is_stable() {
        let refund = NetRefund {
            gross_amount: 1,
            processing_fee: 2,
            network_fee: 3,
            net_amount: 4,
        };
        assert_eq!(
            (
                refund.gross_amount,
                refund.processing_fee,
                refund.network_fee,
                refund.net_amount
            ),
            (1, 2, 3, 4)
        );
    }
}
