//! #881: malicious caller fuzz suite, runnable in the normal test harness.
//!
//! Mirrors `fuzz/fuzz_targets/malicious_caller.rs` with `proptest`, so the same
//! scenarios run under `cargo test` without nightly / cargo-fuzz. Auth is
//! mocked for every address; only the contract's role checks protect the
//! escrow. Random operation sequences are replayed with actors drawn from
//! {admin, merchant, payer, attacker}, and after every step:
//!
//! - no call panics (all calls use `try_*`);
//! - any call by the attacker to an entrypoint reserved for another role fails
//!   and leaves every invoice and the pause flag unchanged;
//! - every stored invoice satisfies `verify_invoice_invariants` (#882).
//!
//! The deterministic tests below pin the individual attacks the property
//! test explores at random.

use invoice::{
    verify_invoice_invariants, Invoice, InvoiceContract, InvoiceContractClient, InvoiceError,
    InvoiceStatus, MaybeAddress, MaybeBytes,
};
use proptest::prelude::*;
use soroban_sdk::{testutils::Address as _, Address, Env, Vec};

const MAX_INVOICES: u64 = 4;
const AMOUNT: i128 = 10_000_000;
const GROSS: i128 = 10_250_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Actor {
    Admin,
    Merchant,
    Payer,
    Attacker,
}

#[derive(Debug, Clone, Copy)]
enum Op {
    Create(Actor),
    MarkPaid(Actor, u8),
    Release(Actor, u8),
    Cancel(Actor, u8),
    Amend(Actor, u8),
    RequestRefund(Actor, u8),
    ApproveRefund(Actor, u8),
    RejectRefund(Actor, u8),
    Transfer(Actor, u8, Actor),
    ExtendExpiry(Actor, u8),
    BatchCancel(Actor, u8, u8),
    BatchExpire(Actor, u8, u8),
    Pause(Actor),
    Unpause(Actor),
}

impl Op {
    fn actor(&self) -> Actor {
        match *self {
            Op::Create(a)
            | Op::MarkPaid(a, _)
            | Op::Release(a, _)
            | Op::Cancel(a, _)
            | Op::Amend(a, _)
            | Op::RequestRefund(a, _)
            | Op::ApproveRefund(a, _)
            | Op::RejectRefund(a, _)
            | Op::Transfer(a, _, _)
            | Op::ExtendExpiry(a, _)
            | Op::BatchCancel(a, _, _)
            | Op::BatchExpire(a, _, _)
            | Op::Pause(a)
            | Op::Unpause(a) => a,
        }
    }
}

fn actor_strategy() -> impl Strategy<Value = Actor> {
    prop_oneof![
        Just(Actor::Admin),
        Just(Actor::Merchant),
        Just(Actor::Payer),
        Just(Actor::Attacker),
    ]
}

fn op_strategy() -> impl Strategy<Value = Op> {
    let a = actor_strategy;
    prop_oneof![
        a().prop_map(Op::Create),
        (a(), any::<u8>()).prop_map(|(x, i)| Op::MarkPaid(x, i)),
        (a(), any::<u8>()).prop_map(|(x, i)| Op::Release(x, i)),
        (a(), any::<u8>()).prop_map(|(x, i)| Op::Cancel(x, i)),
        (a(), any::<u8>()).prop_map(|(x, i)| Op::Amend(x, i)),
        (a(), any::<u8>()).prop_map(|(x, i)| Op::RequestRefund(x, i)),
        (a(), any::<u8>()).prop_map(|(x, i)| Op::ApproveRefund(x, i)),
        (a(), any::<u8>()).prop_map(|(x, i)| Op::RejectRefund(x, i)),
        (a(), any::<u8>(), a()).prop_map(|(x, i, t)| Op::Transfer(x, i, t)),
        (a(), any::<u8>()).prop_map(|(x, i)| Op::ExtendExpiry(x, i)),
        (a(), any::<u8>(), any::<u8>()).prop_map(|(x, i, j)| Op::BatchCancel(x, i, j)),
        (a(), any::<u8>(), any::<u8>()).prop_map(|(x, i, j)| Op::BatchExpire(x, i, j)),
        a().prop_map(Op::Pause),
        a().prop_map(Op::Unpause),
    ]
}

struct Ctx {
    env: Env,
    client: InvoiceContractClient<'static>,
    admin: Address,
    merchant: Address,
    payer: Address,
    attacker: Address,
}

fn setup() -> Ctx {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let merchant = Address::generate(&env);
    let payer = Address::generate(&env);
    let attacker = Address::generate(&env);
    let contract_id = env.register(InvoiceContract, ());
    let client = InvoiceContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    Ctx {
        env,
        client,
        admin,
        merchant,
        payer,
        attacker,
    }
}

impl Ctx {
    fn addr(&self, a: Actor) -> Address {
        match a {
            Actor::Admin => self.admin.clone(),
            Actor::Merchant => self.merchant.clone(),
            Actor::Payer => self.payer.clone(),
            Actor::Attacker => self.attacker.clone(),
        }
    }

    fn id(&self, raw: u8) -> u64 {
        u64::from(raw) % (MAX_INVOICES + 1)
    }

    fn snapshot(&self) -> std::vec::Vec<Option<Invoice>> {
        (1..=MAX_INVOICES)
            .map(|id| self.client.try_get_invoice(&id).ok().and_then(|r| r.ok()))
            .collect()
    }

    fn ids(&self, a: u8, b: u8) -> Vec<u64> {
        let mut v = Vec::new(&self.env);
        v.push_back(self.id(a));
        v.push_back(self.id(b));
        v
    }

    /// Executes `op`, returning whether it succeeded.
    fn run(&self, op: Op) -> bool {
        let c = &self.client;
        match op {
            Op::Create(a) => c
                .try_create_invoice(
                    &self.addr(a),
                    &AMOUNT,
                    &GROSS,
                    &3600,
                    &MaybeBytes::None,
                    &MaybeBytes::None,
                    &0,
                    &MaybeAddress::None,
                )
                .is_ok_and(|r| r.is_ok()),
            Op::MarkPaid(a, i) => c
                .try_mark_paid(
                    &self.addr(a),
                    &self.id(i),
                    &self.payer,
                    &MaybeBytes::None,
                    &MaybeAddress::None,
                )
                .is_ok_and(|r| r.is_ok()),
            Op::Release(a, i) => c
                .try_release_escrow(&self.addr(a), &self.id(i))
                .is_ok_and(|r| r.is_ok()),
            Op::Cancel(a, i) => c
                .try_cancel_invoice(&self.addr(a), &self.id(i))
                .is_ok_and(|r| r.is_ok()),
            Op::Amend(a, i) => c
                .try_amend_invoice(&self.addr(a), &self.id(i), &AMOUNT, &GROSS, &7200)
                .is_ok_and(|r| r.is_ok()),
            Op::RequestRefund(a, i) => c
                .try_request_refund(&self.addr(a), &self.id(i))
                .is_ok_and(|r| r.is_ok()),
            Op::ApproveRefund(a, i) => c
                .try_approve_refund(&self.addr(a), &self.id(i))
                .is_ok_and(|r| r.is_ok()),
            Op::RejectRefund(a, i) => c
                .try_reject_refund(&self.addr(a), &self.id(i))
                .is_ok_and(|r| r.is_ok()),
            Op::Transfer(a, i, t) => c
                .try_transfer_invoice(&self.addr(a), &self.id(i), &self.addr(t))
                .is_ok_and(|r| r.is_ok()),
            Op::ExtendExpiry(a, i) => c
                .try_extend_expiry(&self.addr(a), &self.id(i), &60)
                .is_ok_and(|r| r.is_ok()),
            Op::BatchCancel(a, i, j) => c
                .try_batch_cancel_invoices(&self.addr(a), &self.ids(i, j), &MaybeBytes::None)
                .is_ok_and(|r| r.is_ok()),
            Op::BatchExpire(a, i, j) => c
                .try_batch_expire(&self.addr(a), &self.ids(i, j))
                .is_ok_and(|r| r.is_ok()),
            Op::Pause(a) => c.try_pause(&self.addr(a)).is_ok_and(|r| r.is_ok()),
            Op::Unpause(a) => c.try_unpause(&self.addr(a)).is_ok_and(|r| r.is_ok()),
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn malicious_callers_cannot_change_state(ops in prop::collection::vec(op_strategy(), 1..16)) {
        let ctx = setup();
        for op in ops {
            match op {
                Op::Create(_) if ctx.client.get_invoice_count() >= MAX_INVOICES => continue,
                // An attacker-created invoice, or a payer handing an invoice to
                // the attacker, would make the attacker a legitimate party.
                Op::Create(Actor::Attacker) | Op::Transfer(_, _, Actor::Attacker) => continue,
                _ => {}
            }

            let before = ctx.snapshot();
            let paused_before = ctx.client.is_paused();
            let succeeded = ctx.run(op);

            if op.actor() == Actor::Attacker {
                prop_assert!(!succeeded, "attacker succeeded at {:?}", op);
                prop_assert_eq!(&before, &ctx.snapshot(), "attacker changed state via {:?}", op);
                prop_assert_eq!(paused_before, ctx.client.is_paused());
            }

            for stored in ctx.snapshot().into_iter().flatten() {
                prop_assert_eq!(verify_invoice_invariants(&stored), Ok(()));
            }
        }
    }
}

// ── Deterministic attack scenarios ─────────────────────────────────────────

fn pending(ctx: &Ctx) -> u64 {
    ctx.client.create_invoice(
        &ctx.merchant,
        &AMOUNT,
        &GROSS,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
        &MaybeAddress::None,
    )
}

fn paid(ctx: &Ctx) -> u64 {
    let id = pending(ctx);
    ctx.client
        .mark_paid(&ctx.admin, &id, &ctx.payer, &MaybeBytes::None, &MaybeAddress::None);
    id
}

#[test]
fn attacker_cannot_mark_paid_or_release() {
    let ctx = setup();
    let id = pending(&ctx);
    assert_eq!(
        ctx.client
            .try_mark_paid(&ctx.attacker, &id, &ctx.attacker, &MaybeBytes::None, &MaybeAddress::None)
            .unwrap_err()
            .unwrap(),
        InvoiceError::Unauthorized
    );
    let id = paid(&ctx);
    assert_eq!(
        ctx.client.try_release_escrow(&ctx.attacker, &id).unwrap_err().unwrap(),
        InvoiceError::Unauthorized
    );
    assert_eq!(ctx.client.get_invoice(&id).status, InvoiceStatus::Paid);
}

#[test]
fn attacker_cannot_cancel_or_amend_someone_elses_invoice() {
    let ctx = setup();
    let id = pending(&ctx);
    assert_eq!(
        ctx.client.try_cancel_invoice(&ctx.attacker, &id).unwrap_err().unwrap(),
        InvoiceError::Unauthorized
    );
    assert_eq!(
        ctx.client
            .try_amend_invoice(&ctx.attacker, &id, &AMOUNT, &GROSS, &7200)
            .unwrap_err()
            .unwrap(),
        InvoiceError::Unauthorized
    );
    let mut ids = Vec::new(&ctx.env);
    ids.push_back(id);
    assert_eq!(
        ctx.client
            .try_batch_cancel_invoices(&ctx.attacker, &ids, &MaybeBytes::None)
            .unwrap_err()
            .unwrap(),
        InvoiceError::Unauthorized
    );
    assert_eq!(ctx.client.get_invoice(&id).status, InvoiceStatus::Pending);
}

#[test]
fn attacker_cannot_hijack_refund_or_payer_rights() {
    let ctx = setup();
    let id = paid(&ctx);
    assert_eq!(
        ctx.client.try_request_refund(&ctx.attacker, &id).unwrap_err().unwrap(),
        InvoiceError::Unauthorized
    );
    assert_eq!(
        ctx.client
            .try_transfer_invoice(&ctx.attacker, &id, &ctx.attacker)
            .unwrap_err()
            .unwrap(),
        InvoiceError::Unauthorized
    );
    ctx.client.request_refund(&ctx.payer, &id);
    assert_eq!(
        ctx.client.try_approve_refund(&ctx.attacker, &id).unwrap_err().unwrap(),
        InvoiceError::Unauthorized
    );
    assert_eq!(
        ctx.client.try_reject_refund(&ctx.attacker, &id).unwrap_err().unwrap(),
        InvoiceError::Unauthorized
    );
    let stored = ctx.client.get_invoice(&id);
    assert_eq!(stored.status, InvoiceStatus::RefundRequested);
    assert_eq!(stored.payer, MaybeAddress::Some(ctx.payer.clone()));
}

#[test]
fn attacker_cannot_pause_or_reconfigure() {
    let ctx = setup();
    let id = pending(&ctx);
    assert_eq!(
        ctx.client.try_pause(&ctx.attacker).unwrap_err().unwrap(),
        InvoiceError::Unauthorized
    );
    assert_eq!(
        ctx.client.try_extend_expiry(&ctx.attacker, &id, &60).unwrap_err().unwrap(),
        InvoiceError::Unauthorized
    );
    ctx.client.pause(&ctx.admin);
    assert_eq!(
        ctx.client.try_unpause(&ctx.attacker).unwrap_err().unwrap(),
        InvoiceError::Unauthorized
    );
    assert!(ctx.client.is_paused());
}

/// #882 + #881: a payer cannot launder an invoice back to the merchant, which
/// would recreate the self-pay state `mark_paid` forbids.
#[test]
fn transfer_to_merchant_is_rejected_by_invariants() {
    let ctx = setup();
    let id = paid(&ctx);
    assert_eq!(
        ctx.client
            .try_transfer_invoice(&ctx.payer, &id, &ctx.merchant)
            .unwrap_err()
            .unwrap(),
        InvoiceError::InvariantViolation
    );
    assert_eq!(
        ctx.client.get_invoice(&id).payer,
        MaybeAddress::Some(ctx.payer.clone())
    );
}
