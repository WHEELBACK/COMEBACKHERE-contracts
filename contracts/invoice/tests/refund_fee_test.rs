//! #71: net refund payout after deducting processing and network fees.
//!
//! A refund does not hand the customer back the gross amount. The merchant's
//! payment gateway takes a share (`fee_bps`) and the payout itself costs a flat
//! network fee, so `process_refund` transfers `net_amount` and reports the whole
//! gross-vs-net breakdown.
//!
//! What these tests hold the implementation to:
//!
//! - the transferred amount is the `net_amount` of the reported breakdown, to
//!   the stroop — an indexer reading `refund_processed` must never see a
//!   different number from the one that moved;
//! - the payout, the stored record and the published event are all the *same*
//!   `NetRefund`, so the three consumers cannot drift apart;
//! - an out-of-range `fee_bps` is refused **before** the token is called, so a
//!   bad fee policy can never produce a partial or mispriced payout;
//! - the read-only preview agrees with what a real refund applies, which is what
//!   lets a merchant quote a customer before approving.

use invoice::{
    InvoiceContract, InvoiceContractClient, InvoiceError, InvoiceStatus, MaybeAddress, MaybeBytes,
    NetRefund, RefundProcessedEvent, REFUND_NETWORK_FEE,
};
use soroban_sdk::{
    testutils::{Address as _, Events as _},
    token, Address, Env, Symbol, TryFromVal,
};

/// A minimal SEP-41 token: `process_refund` only ever needs `transfer`.
mod test_token {
    use soroban_sdk::{contract, contracterror, contractimpl, panic_with_error, Address, Env};

    #[contracterror]
    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    #[repr(u32)]
    pub enum TokenError {
        TransferFailed = 1,
    }

    #[contract]
    pub struct TestToken;

    #[contractimpl]
    impl TestToken {
        pub fn mint(env: Env, to: Address, amount: i128) {
            let key = ("bal", to.clone());
            let bal: i128 = env.storage().persistent().get(&key).unwrap_or(0);
            env.storage().persistent().set(&key, &(bal + amount));
        }

        pub fn balance(env: Env, of: Address) -> i128 {
            let key = ("bal", of);
            env.storage().persistent().get(&key).unwrap_or(0)
        }

        pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
            from.require_auth();
            let key = ("bal", from.clone());
            let bal: i128 = env.storage().persistent().get(&key).unwrap_or(0);
            if bal < amount {
                panic_with_error!(&env, TokenError::TransferFailed);
            }
            env.storage().persistent().set(&key, &(bal - amount));
            let to_key = ("bal", to);
            let to_bal: i128 = env.storage().persistent().get(&to_key).unwrap_or(0);
            env.storage().persistent().set(&to_key, &(to_bal + amount));
        }
    }
}

use test_token::{TestToken, TestTokenClient};

const AMOUNT: i128 = 10_000_000;
const GROSS: i128 = 10_250_000;

/// 2.5% — an ordinary card-scheme processing rate.
const FEE_BPS: u32 = 250;

/// The smallest amount `create_invoice` accepts: 1 USDC in stroops, which is
/// what `require_usdc_precision` enforces.
const USDC_FACTOR: i128 = 10_000_000;

struct Fixture {
    env: Env,
    admin: Address,
    merchant: Address,
    payer: Address,
    invoice_id: Address,
    invoice: InvoiceContractClient<'static>,
    token_id: Address,
}

fn setup() -> Fixture {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let merchant = Address::generate(&env);
    let payer = Address::generate(&env);
    let token_id = env.register(TestToken, ());

    let invoice_id = env.register(InvoiceContract, ());
    let invoice = InvoiceContractClient::new(&env, &invoice_id);
    invoice.initialize(&admin);

    Fixture {
        env,
        admin,
        merchant,
        payer,
        invoice_id,
        invoice,
        token_id,
    }
}

/// Creates a paid, disputed invoice with `token_address` set and escrow funded,
/// leaving it in `RefundRequested` and ready to be paid out.
fn paid_disputed_and_funded(f: &Fixture) -> u64 {
    let id = f.invoice.create_invoice(
        &f.merchant,
        &AMOUNT,
        &GROSS,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
        &MaybeAddress::Some(f.token_id.clone()),
    );
    f.invoice.mark_paid(
        &f.admin,
        &id,
        &f.payer,
        &MaybeBytes::None,
        &MaybeAddress::Some(f.token_id.clone()),
    );
    f.invoice.request_refund(&f.payer, &id);
    TestTokenClient::new(&f.env, &f.token_id).mint(&f.invoice_id, &AMOUNT);
    id
}

fn token_balance(f: &Fixture, of: &Address) -> i128 {
    TestTokenClient::new(&f.env, &f.token_id).balance(of)
}

/// The symbol that named the last event published in the current top-level call.
/// Read straight after the call, because soroban-sdk 22 no longer accumulates
/// events across top-level calls.
fn last_event_symbol(env: &Env) -> Symbol {
    let (_, topics, _) = env.events().all().last().unwrap();
    Symbol::try_from_val(env, &topics.get_unchecked(0)).unwrap()
}

/// The id topic of the last event published in the current top-level call.
fn last_event_id(env: &Env) -> u64 {
    let (_, topics, _) = env.events().all().last().unwrap();
    u64::try_from_val(env, &topics.get_unchecked(1)).unwrap()
}

/// The payload of the last event published in the current top-level call.
fn last_event_payload(env: &Env) -> RefundProcessedEvent {
    let (_, _, data) = env.events().all().last().unwrap();
    RefundProcessedEvent::try_from_val(env, &data)
        .expect("refund_processed must publish a RefundProcessedEvent")
}

// ── the payout is the net amount, and the reported breakdown describes it ─────

/// The core of #71: the payer receives `gross - processing - network`, the
/// invoice is marked `Refunded`, and the retained escrow is exactly the two
/// fees. If the transfer were the gross, the protocol would pay the fees out of
/// its own balance instead of deducting them.
#[test]
fn the_payer_receives_the_net_amount_and_the_escrow_keeps_the_fees() {
    let f = setup();
    let id = paid_disputed_and_funded(&f);

    let refund = f.invoice.process_refund(&f.admin, &id, &FEE_BPS);

    let expected_processing = AMOUNT * FEE_BPS as i128 / 10_000;
    assert_eq!(
        refund,
        NetRefund {
            gross_amount: AMOUNT,
            processing_fee: expected_processing,
            network_fee: REFUND_NETWORK_FEE,
            net_amount: AMOUNT - expected_processing - REFUND_NETWORK_FEE,
        }
    );
    assert_eq!(
        token_balance(&f, &f.payer),
        AMOUNT - expected_processing - REFUND_NETWORK_FEE,
        "the transferred amount must be the net amount, not the gross"
    );
    assert_eq!(
        token_balance(&f, &f.invoice_id),
        expected_processing + REFUND_NETWORK_FEE,
        "the escrow retains exactly the fees it kept"
    );
    assert_eq!(f.invoice.get_invoice(&id).status, InvoiceStatus::Refunded);
}

/// The stored breakdown and the returned value are the same record, so an
/// off-chain reader that never saw the transaction can still reconcile the
/// payout afterwards.
#[test]
fn the_breakdown_is_stored_for_the_invoice() {
    let f = setup();
    let id = paid_disputed_and_funded(&f);

    assert_eq!(
        f.invoice.try_get_refund_breakdown(&id),
        Err(Ok(InvoiceError::NotFound)),
        "an approved-but-unpaid refund has no breakdown yet"
    );

    let applied = f.invoice.process_refund(&f.admin, &id, &FEE_BPS);
    assert_eq!(f.invoice.get_refund_breakdown(&id), applied);
}

/// Two different fee policies on two invoices must be reported independently —
/// a single shared slot would let the second refund overwrite the first.
#[test]
fn breakdowns_are_tracked_per_invoice() {
    let f = setup();
    let first = paid_disputed_and_funded(&f);
    let second = paid_disputed_and_funded(&f);

    let full = f.invoice.process_refund(&f.admin, &first, &0);
    let charged = f.invoice.process_refund(&f.admin, &second, &FEE_BPS);

    assert_eq!(full.processing_fee, 0);
    assert!(charged.processing_fee > 0);
    assert_eq!(f.invoice.get_refund_breakdown(&first), full);
    assert_eq!(f.invoice.get_refund_breakdown(&second), charged);
}

/// The `refund_processed` event is what an indexer reconciles against, so it
/// must carry the identical breakdown — including the invoice id in its topics.
#[test]
fn the_refund_processed_event_carries_the_breakdown() {
    let f = setup();
    let id = paid_disputed_and_funded(&f);

    let applied = f.invoice.process_refund(&f.admin, &id, &FEE_BPS);

    assert_eq!(
        last_event_symbol(&f.env),
        Symbol::new(&f.env, "refund_processed")
    );
    assert_eq!(last_event_id(&f.env), id);

    let published = last_event_payload(&f.env);
    assert_eq!(published.id, id);
    assert_eq!(published.payer, f.payer);
    assert_eq!(published.gross_amount, applied.gross_amount);
    assert_eq!(published.processing_fee, applied.processing_fee);
    assert_eq!(published.network_fee, applied.network_fee);
    assert_eq!(
        published.net_amount, applied.net_amount,
        "the event must report the amount that was actually transferred"
    );
}

// ── a bad fee policy cannot produce a payout ─────────────────────────────────

/// A fee above 100% is refused with a typed error, and refused *before* the
/// token is touched: the invoice stays `RefundRequested` and the escrow balance
/// is unchanged, so the refund remains retryable under a valid policy.
#[test]
fn an_out_of_range_fee_is_refused_before_any_transfer() {
    let f = setup();
    let id = paid_disputed_and_funded(&f);

    assert_eq!(
        f.invoice.try_process_refund(&f.admin, &id, &(10_001)),
        Err(Ok(InvoiceError::RefundFeeTooHigh))
    );
    assert_eq!(
        f.invoice.try_process_refund(&f.admin, &id, &u32::MAX),
        Err(Ok(InvoiceError::RefundFeeTooHigh))
    );

    assert_eq!(
        f.invoice.get_invoice(&id).status,
        InvoiceStatus::RefundRequested
    );
    assert_eq!(token_balance(&f, &f.invoice_id), AMOUNT, "escrow untouched");
    assert_eq!(token_balance(&f, &f.payer), 0, "payer paid nothing");
    assert_eq!(
        f.invoice.try_get_refund_breakdown(&id),
        Err(Ok(InvoiceError::NotFound)),
        "a refused refund must not leave a breakdown behind"
    );

    // Still retryable under a valid policy.
    let applied = f.invoice.process_refund(&f.admin, &id, &FEE_BPS);
    assert_eq!(f.invoice.get_refund_breakdown(&id), applied);
}

// ── the preview agrees with the payout ───────────────────────────────────────

/// The view entrypoint is what a merchant UI quotes from, so it must return
/// exactly what `process_refund` then applies. If these ever diverged, a quoted
/// refund would not be the delivered refund.
#[test]
fn the_preview_matches_the_applied_breakdown() {
    let f = setup();

    for fee_bps in [0_u32, 1, 25, FEE_BPS, 1_000, 5_000, 9_999, 10_000] {
        let quoted = f.invoice.calculate_net_refund(&AMOUNT, &fee_bps);
        let id = paid_disputed_and_funded(&f);
        let applied = f.invoice.process_refund(&f.admin, &id, &fee_bps);
        assert_eq!(quoted, applied, "quote diverged for {fee_bps} bps");
    }
}

/// The preview is a pure function of its arguments and is deliberately not
/// pause-gated: an operator working out what a paused contract owes its
/// customers is exactly the case where the read has to keep working.
#[test]
fn the_preview_works_while_the_contract_is_paused() {
    let f = setup();
    let before = f.invoice.calculate_net_refund(&AMOUNT, &FEE_BPS);
    f.invoice.pause(&f.admin);
    assert_eq!(f.invoice.calculate_net_refund(&AMOUNT, &FEE_BPS), before);
}

#[test]
fn the_preview_rejects_the_same_inputs_the_payout_does() {
    let f = setup();
    assert_eq!(
        f.invoice.try_calculate_net_refund(&AMOUNT, &10_001),
        Err(Ok(InvoiceError::RefundFeeTooHigh))
    );
    assert_eq!(
        f.invoice.try_calculate_net_refund(&0, &FEE_BPS),
        Err(Ok(InvoiceError::InvalidAmount))
    );
}

// ── a 100% fee policy is a legal, if degenerate, choice ─────────────────────

/// A merchant who charges 100% is entitled to it: the refund completes, the
/// payer receives nothing, and no tokens leave the escrow. What must not happen
/// is a negative transfer or a stuck `RefundRequested`.
#[test]
fn a_full_fee_completes_the_refund_without_moving_tokens() {
    let f = setup();
    let id = paid_disputed_and_funded(&f);

    let applied = f.invoice.process_refund(&f.admin, &id, &10_000);

    assert_eq!(applied.processing_fee, AMOUNT);
    assert_eq!(applied.net_amount, 0);
    assert_eq!(token_balance(&f, &f.payer), 0);
    assert_eq!(token_balance(&f, &f.invoice_id), AMOUNT);
    assert_eq!(f.invoice.get_invoice(&id).status, InvoiceStatus::Refunded);
}

/// `REFUND_NETWORK_FEE` is deliberately far below `USDC_FACTOR`, the smallest
/// amount `create_invoice` accepts, so the network fee can never silently
/// consume a real invoice: even the cheapest refundable invoice still pays the
/// customer something. The `net_amount == 0` floor exists for callers of the
/// arithmetic directly, and is pinned by the unit tests in `src/refund.rs`.
#[test]
fn the_smallest_creatable_invoice_still_pays_a_positive_net_amount() {
    // A protocol constant, not a test expectation: if the network fee ever
    // reached the minimum invoice size this would stop compiling.
    const { assert!(REFUND_NETWORK_FEE < USDC_FACTOR) };

    let f = setup();
    let id = f.invoice.create_invoice(
        &f.merchant,
        &USDC_FACTOR,
        &GROSS,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
        &MaybeAddress::Some(f.token_id.clone()),
    );
    f.invoice.mark_paid(
        &f.admin,
        &id,
        &f.payer,
        &MaybeBytes::None,
        &MaybeAddress::Some(f.token_id.clone()),
    );
    f.invoice.request_refund(&f.payer, &id);
    TestTokenClient::new(&f.env, &f.token_id).mint(&f.invoice_id, &USDC_FACTOR);

    let applied = f.invoice.process_refund(&f.admin, &id, &0);

    assert_eq!(applied.gross_amount, USDC_FACTOR);
    assert!(
        applied.net_amount > 0,
        "the smallest refundable invoice must still pay out: {applied:?}"
    );
    assert_eq!(token_balance(&f, &f.payer), applied.net_amount);
}

// ── the deduction comes out of the refund, never out of the escrow ───────────

/// The token balance the payout is measured against is the invoice contract's
/// own escrow, and the fee never widens it: the net payout plus the retained
/// fees always reconcile to the gross, with nothing minted or burned.
#[test]
fn the_escrow_reconciles_exactly_across_fee_policies() {
    for fee_bps in [0_u32, 1, 25, FEE_BPS, 1_000, 5_000, 9_999, 10_000] {
        let f = setup();
        let id = paid_disputed_and_funded(&f);

        let applied = f.invoice.process_refund(&f.admin, &id, &fee_bps);
        let paid_out = token_balance(&f, &f.payer);
        let retained = token_balance(&f, &f.invoice_id);

        assert_eq!(paid_out, applied.net_amount, "fee_bps={fee_bps}");
        assert_eq!(
            paid_out + retained,
            AMOUNT,
            "fee_bps={fee_bps}: tokens were created or destroyed"
        );
        // The escrow keeps what the payout did not take. When the fees exceed
        // the gross the payout floors at zero, so the retained amount is capped
        // at the gross rather than equal to the nominal fee total.
        let expected_retained = if applied.processing_fee + applied.network_fee >= AMOUNT {
            AMOUNT
        } else {
            applied.processing_fee + applied.network_fee
        };
        assert_eq!(retained, expected_retained, "fee_bps={fee_bps}");
    }
}

/// Guards the assumption every assertion above rests on: the token double
/// really is SEP-41-shaped, so `transfer_net_refund` is talking to a token and
/// not to an invoice-shaped address.
#[test]
fn the_escrow_is_a_sep41_token() {
    let f = setup();
    let client = token::Client::new(&f.env, &f.token_id);
    assert_eq!(client.balance(&f.invoice_id), 0);
}
