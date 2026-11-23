#![no_main]

//! Fuzz harness: malicious caller scenarios against the invoice contract (#881).
//!
//! # Threat model
//!
//! Auth is mocked for every address (`mock_all_auths`), so `require_auth`
//! always passes. What is left standing between an attacker and the escrow is
//! the contract's own *role* checks: admin-only entrypoints compare against the
//! stored admin, merchant entrypoints against `invoice.merchant`, payer
//! entrypoints against `invoice.payer`. This harness replays an arbitrary
//! sequence of operations where every call is made by an actor drawn from a
//! small pool (admin, merchant, payer, attacker) and checks, after each step:
//!
//! 1. **No panics.** Every call goes through `try_*`; any input must produce
//!    `Ok` or a typed `Err`, never a trap.
//! 2. **Role checks hold.** Whenever the attacker calls an entrypoint reserved
//!    for another role, the call fails and no invoice changes.
//! 3. **State invariants hold.** Every stored invoice still satisfies
//!    `verify_invoice_invariants` (#882), whatever sequence produced it.
//!
//! # Cost budget
//!
//! `MAX_OPS` and `MAX_INVOICES` are small so a bounded `cargo fuzz run`
//! explores many short sequences instead of a few long ones.

use arbitrary::Arbitrary;
use invoice::{
    verify_invoice_invariants, Invoice, InvoiceContract, InvoiceContractClient, MaybeAddress,
    MaybeBytes,
};
use libfuzzer_sys::fuzz_target;
use soroban_sdk::{testutils::Address as _, Address, Env, Vec};

const MAX_OPS: usize = 16;
const MAX_INVOICES: u64 = 4;
const AMOUNT: i128 = 10_000_000;
const GROSS: i128 = 10_250_000;

#[derive(Arbitrary, Debug, Clone, Copy, PartialEq, Eq)]
enum Actor {
    Admin,
    Merchant,
    Payer,
    Attacker,
}

#[derive(Arbitrary, Debug, Clone, Copy)]
enum Op {
    Create { actor: Actor },
    MarkPaid { actor: Actor, id: u8 },
    Release { actor: Actor, id: u8 },
    Cancel { actor: Actor, id: u8 },
    Amend { actor: Actor, id: u8, bump: u32 },
    RequestRefund { actor: Actor, id: u8 },
    ApproveRefund { actor: Actor, id: u8 },
    RejectRefund { actor: Actor, id: u8 },
    Transfer { actor: Actor, id: u8, to: Actor },
    ExtendExpiry { actor: Actor, id: u8, secs: u32 },
    BatchCancel { actor: Actor, ids: [u8; 2] },
    BatchExpire { actor: Actor, ids: [u8; 2] },
    Pause { actor: Actor },
    Unpause { actor: Actor },
}

#[derive(Arbitrary, Debug)]
struct Input {
    ops: std::vec::Vec<Op>,
}

struct Ctx {
    env: Env,
    client: InvoiceContractClient<'static>,
    admin: Address,
    merchant: Address,
    payer: Address,
    attacker: Address,
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
}

/// Whether `op` is one the attacker can never be entitled to perform: every
/// admin-only entrypoint, and every merchant/payer entrypoint (the attacker
/// never creates invoices as merchant nor pays as payer in this model).
fn attacker_must_fail(op: &Op) -> bool {
    match *op {
        Op::Create { .. } => false, // anyone may create their own invoice
        Op::MarkPaid { actor, .. }
        | Op::Release { actor, .. }
        | Op::Cancel { actor, .. }
        | Op::Amend { actor, .. }
        | Op::RequestRefund { actor, .. }
        | Op::ApproveRefund { actor, .. }
        | Op::RejectRefund { actor, .. }
        | Op::Transfer { actor, .. }
        | Op::ExtendExpiry { actor, .. }
        | Op::BatchCancel { actor, .. }
        | Op::BatchExpire { actor, .. }
        | Op::Pause { actor }
        | Op::Unpause { actor } => actor == Actor::Attacker,
    }
}

fn run(ctx: &Ctx, op: Op) -> bool {
    let c = &ctx.client;
    match op {
        Op::Create { actor } => c
            .try_create_invoice(
                &ctx.addr(actor),
                &AMOUNT,
                &GROSS,
                &3600,
                &MaybeBytes::None,
                &MaybeBytes::None,
                &0,
                &MaybeAddress::None,
            )
            .is_ok_and(|r| r.is_ok()),
        Op::MarkPaid { actor, id } => c
            .try_mark_paid(
                &ctx.addr(actor),
                &ctx.id(id),
                &ctx.payer,
                &MaybeBytes::None,
                &MaybeAddress::None,
            )
            .is_ok_and(|r| r.is_ok()),
        Op::Release { actor, id } => c
            .try_release_escrow(&ctx.addr(actor), &ctx.id(id))
            .is_ok_and(|r| r.is_ok()),
        Op::Cancel { actor, id } => c
            .try_cancel_invoice(&ctx.addr(actor), &ctx.id(id))
            .is_ok_and(|r| r.is_ok()),
        Op::Amend { actor, id, bump } => c
            .try_amend_invoice(
                &ctx.addr(actor),
                &ctx.id(id),
                &(AMOUNT + i128::from(bump)),
                &(GROSS + i128::from(bump)),
                &3600,
            )
            .is_ok_and(|r| r.is_ok()),
        Op::RequestRefund { actor, id } => c
            .try_request_refund(&ctx.addr(actor), &ctx.id(id))
            .is_ok_and(|r| r.is_ok()),
        Op::ApproveRefund { actor, id } => c
            .try_approve_refund(&ctx.addr(actor), &ctx.id(id))
            .is_ok_and(|r| r.is_ok()),
        Op::RejectRefund { actor, id } => c
            .try_reject_refund(&ctx.addr(actor), &ctx.id(id))
            .is_ok_and(|r| r.is_ok()),
        Op::Transfer { actor, id, to } => c
            .try_transfer_invoice(&ctx.addr(actor), &ctx.id(id), &ctx.addr(to))
            .is_ok_and(|r| r.is_ok()),
        Op::ExtendExpiry { actor, id, secs } => c
            .try_extend_expiry(&ctx.addr(actor), &ctx.id(id), &u64::from(secs))
            .is_ok_and(|r| r.is_ok()),
        Op::BatchCancel { actor, ids } => {
            let mut v = Vec::new(&ctx.env);
            for raw in ids {
                v.push_back(ctx.id(raw));
            }
            c.try_batch_cancel_invoices(&ctx.addr(actor), &v, &MaybeBytes::None)
                .is_ok_and(|r| r.is_ok())
        }
        Op::BatchExpire { actor, ids } => {
            let mut v = Vec::new(&ctx.env);
            for raw in ids {
                v.push_back(ctx.id(raw));
            }
            c.try_batch_expire(&ctx.addr(actor), &v)
                .is_ok_and(|r| r.is_ok())
        }
        Op::Pause { actor } => c.try_pause(&ctx.addr(actor)).is_ok_and(|r| r.is_ok()),
        Op::Unpause { actor } => c.try_unpause(&ctx.addr(actor)).is_ok_and(|r| r.is_ok()),
    }
}

fuzz_target!(|input: Input| {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let merchant = Address::generate(&env);
    let payer = Address::generate(&env);
    let attacker = Address::generate(&env);
    let contract_id = env.register_contract(None, InvoiceContract);
    let client = InvoiceContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let ctx = Ctx {
        env,
        client,
        admin,
        merchant,
        payer,
        attacker,
    };

    for op in input.ops.into_iter().take(MAX_OPS) {
        // Bound the id space: never create more than MAX_INVOICES.
        if matches!(op, Op::Create { .. }) && ctx.client.get_invoice_count() >= MAX_INVOICES {
            continue;
        }
        // An attacker-created invoice makes the attacker a legitimate
        // merchant of that invoice, which this model does not track.
        if matches!(op, Op::Create { actor: Actor::Attacker }) {
            continue;
        }
        // Likewise, a legitimate payer handing the invoice to the attacker
        // makes the attacker the rightful payer from then on.
        if matches!(op, Op::Transfer { to: Actor::Attacker, .. }) {
            continue;
        }

        let before = ctx.snapshot();
        let paused_before = ctx.client.is_paused();
        let succeeded = run(&ctx, op);

        if attacker_must_fail(&op) {
            assert!(!succeeded, "attacker succeeded at {op:?}");
            assert_eq!(before, ctx.snapshot(), "attacker changed state via {op:?}");
            assert_eq!(paused_before, ctx.client.is_paused());
        }

        for stored in ctx.snapshot().into_iter().flatten() {
            assert_eq!(
                verify_invoice_invariants(&stored),
                Ok(()),
                "invariant broken after {op:?}: {stored:?}"
            );
        }
    }
});
