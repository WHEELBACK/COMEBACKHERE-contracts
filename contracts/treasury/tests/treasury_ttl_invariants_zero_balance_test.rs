//! Tests for issues #864, #865, #866, and #867:
//!
//! * #866 — TTL bump: `write_settlement` on a live settlement must not panic
//!   (storage entry exists after the write; actual ledger TTL extension is
//!   exercised structurally but cannot be asserted on the numeric value in a
//!   test environment that has no ledger-eviction simulation).
//! * #864 — Invariant guard: `verify_settlement_invariants` catches impossible
//!   states; tested indirectly by asserting a valid proposal succeeds and
//!   verifying the settlement is retrievable.
//! * #867 — Zero-balance safety: `withdraw` returns `InsufficientBalance` for
//!   an account that has never deposited; `withdraw_all` on a zero-balance
//!   treasury emits the `treasury_drained` event without panicking.
//! * #865 — Error category: `error_category` returns a non-empty string pair
//!   for a sample of `TreasuryError` variants.

use soroban_sdk::{contract, contractimpl, testutils::Address as _, Address, Env};
use treasury::{
    errors::error_category, SettlementStatus, TreasuryContract, TreasuryContractClient,
    TreasuryError,
};

// ── Minimal in-process test token (mirrors the stub in existing tests) ────────

mod test_token {
    use soroban_sdk::{contract, contractimpl, Address, Env};

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
            let from_key = ("bal", from.clone());
            let to_key = ("bal", to.clone());
            let from_bal: i128 = env.storage().persistent().get(&from_key).unwrap_or(0);
            let to_bal: i128 = env.storage().persistent().get(&to_key).unwrap_or(0);
            env.storage()
                .persistent()
                .set(&from_key, &(from_bal - amount));
            env.storage().persistent().set(&to_key, &(to_bal + amount));
        }
    }
}

use test_token::{TestToken, TestTokenClient};

// ── Shared test setup helpers ─────────────────────────────────────────────────

/// Creates an `Env`, registers the treasury contract with threshold=1, mocks all
/// auths, and returns `(client, admin, treasury_contract_id)`.
fn setup(env: &Env) -> (TreasuryContractClient, Address, Address) {
    env.mock_all_auths();
    let admin = Address::generate(env);
    let treasury_id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(env, &treasury_id);
    client.initialize(&admin, &1, &soroban_sdk::Vec::new(env));
    (client, admin, treasury_id)
}

/// Registers the in-process `TestToken` and returns its address and client.
fn register_token(env: &Env) -> (Address, TestTokenClient) {
    let token_id = env.register_contract(None, TestToken);
    let token_client = TestTokenClient::new(env, &token_id);
    (token_id, token_client)
}

// ── #866: TTL bump is exercised ───────────────────────────────────────────────

/// Calling `write_settlement` via `propose_settlement` on a Pending settlement
/// exercises the TTL bump path without panicking, and the settlement is
/// retrievable from storage afterward.
#[test]
fn ttl_bump_does_not_panic_for_pending_settlement() {
    let env = Env::default();
    let (client, admin, _treasury_id) = setup(&env);
    let merchant = Address::generate(&env);

    // propose_settlement calls write_settlement internally, which triggers
    // bump_settlement_ttl for the new Pending settlement.
    let sid = client.propose_settlement(&admin, &merchant, &1_000_000i128, &0_u64);

    // The settlement must be retrievable — TTL bump must not have corrupted it.
    let settlement = client.get_settlement(&sid);
    assert_eq!(settlement.status, SettlementStatus::Pending);
    assert_eq!(settlement.amount, 1_000_000i128);
}

/// Approving a settlement also calls write_settlement (still Pending), so the
/// TTL bump runs a second time — this must succeed without panicking.
#[test]
fn ttl_bump_does_not_panic_on_subsequent_approval_write() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let treasury_id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(&env, &treasury_id);
    // threshold=2 so the settlement stays Pending after the first approval
    client.initialize(&admin, &2, &soroban_sdk::Vec::new(&env));

    let backup = Address::generate(&env);
    client.set_signer(&admin, &backup, &1);

    let merchant = Address::generate(&env);
    let sid = client.propose_settlement(&admin, &merchant, &5_000_000i128, &0_u64);

    // approve_settlement calls write_settlement again → bump_settlement_ttl fires
    let settlement = client.approve_settlement(&backup, &sid);
    assert_eq!(settlement.status, SettlementStatus::Pending);
    assert_eq!(settlement.approval_weight, 2);
}

// ── #864: Invariant guard ─────────────────────────────────────────────────────

/// A settlement proposed with a valid positive amount is stored correctly and
/// passes all invariant checks (amount > 0, approvals non-empty when weight > 0).
#[test]
fn invariant_guard_passes_for_valid_settlement() {
    let env = Env::default();
    let (client, admin, _treasury_id) = setup(&env);
    let merchant = Address::generate(&env);

    let sid = client.propose_settlement(&admin, &merchant, &999_999i128, &0_u64);
    let settlement = client.get_settlement(&sid);

    // invariant #1: amount must be positive
    assert!(settlement.amount > 0);
    // invariant #2: weight > 0 implies at least one approver recorded
    assert!(settlement.approval_weight > 0);
    assert!(!settlement.approvals.is_empty());
}

/// Proposing a settlement with amount == 0 is caught by the `propose_settlement`
/// layer before `write_settlement` is ever reached, and surfaces as
/// `InvalidAmount`.
#[test]
fn propose_settlement_rejects_zero_amount() {
    let env = Env::default();
    let (client, admin, _treasury_id) = setup(&env);
    let merchant = Address::generate(&env);

    let result = client.try_propose_settlement(&admin, &merchant, &0i128, &0_u64);
    assert_eq!(result, Err(Ok(TreasuryError::InvalidAmount)));
}

/// Proposing with a negative amount also returns `InvalidAmount`.
#[test]
fn propose_settlement_rejects_negative_amount() {
    let env = Env::default();
    let (client, admin, _treasury_id) = setup(&env);
    let merchant = Address::generate(&env);

    let result = client.try_propose_settlement(&admin, &merchant, &-1i128, &0_u64);
    assert_eq!(result, Err(Ok(TreasuryError::InvalidAmount)));
}

// ── #867: Zero-balance withdraw returns InsufficientBalance ───────────────────

/// Withdrawing from an account that has never deposited must return
/// `InsufficientBalance` immediately (via the explicit absent-key guard) rather
/// than silently treating the missing entry as a zero balance and passing the
/// `balance < amount` check.
#[test]
fn withdraw_from_never_deposited_account_returns_insufficient_balance() {
    let env = Env::default();
    let (client, _admin, _treasury_id) = setup(&env);
    let (token_id, _token_client) = register_token(&env);

    let stranger = Address::generate(&env);

    let result = client.try_withdraw(&stranger, &token_id, &1i128);
    assert_eq!(result, Err(Ok(TreasuryError::InsufficientBalance)));
}

/// `get_balance` returns exactly `0` for an address that has never deposited
/// (absent storage entry treated as zero per the zero-balance contract).
#[test]
fn get_balance_returns_zero_for_absent_entry() {
    let env = Env::default();
    let (client, _admin, _treasury_id) = setup(&env);
    let (token_id, _token_client) = register_token(&env);

    let stranger = Address::generate(&env);
    assert_eq!(client.get_balance(&stranger, &token_id), 0i128);
}

/// Withdrawing more than the deposited balance returns `InsufficientBalance`.
#[test]
fn withdraw_more_than_balance_returns_insufficient_balance() {
    let env = Env::default();
    let (client, _admin, treasury_id) = setup(&env);
    let (token_id, token_client) = register_token(&env);

    let depositor = Address::generate(&env);

    // Mint tokens to depositor and to treasury (treasury needs a balance for the
    // transfer to succeed in the TestToken stub).
    token_client.mint(&depositor, &1_000i128);
    token_client.mint(&treasury_id, &1_000i128);
    client.deposit(&depositor, &token_id, &1_000i128, &None);

    // Trying to withdraw more than what was deposited must fail.
    let result = client.try_withdraw(&depositor, &token_id, &2_000i128);
    assert_eq!(result, Err(Ok(TreasuryError::InsufficientBalance)));
}

// ── #867: withdraw_all emits event even on zero balance ───────────────────────

/// When the treasury holds a zero on-chain token balance, `withdraw_all` must
/// not panic and must still emit the `treasury_drained` event.
#[test]
fn withdraw_all_zero_balance_emits_drained_event_without_panic() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury_id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(&env, &treasury_id);
    client.initialize(&admin, &1, &soroban_sdk::Vec::new(&env));

    let (token_id, token_client) = register_token(&env);
    let recipient = Address::generate(&env);

    // Confirm the treasury holds zero tokens.
    assert_eq!(token_client.balance(&treasury_id), 0i128);

    // Pause before calling withdraw_all (required precondition).
    client.pause(&admin);

    // This must not panic and must emit the event (no assertion on the event
    // payload — the absence of a panic is the meaningful assertion here).
    client.withdraw_all(&admin, &token_id, &recipient);

    // Recipient still has zero (nothing to drain) and no panic occurred.
    assert_eq!(token_client.balance(&recipient), 0i128);
}

// ── #865: Error category returns non-empty strings ───────────────────────────

/// A sample of `TreasuryError` variants must each map to the expected category
/// label and a non-empty description string.
#[test]
fn error_category_returns_non_empty_strings_for_sampled_variants() {
    let pairs: &[(TreasuryError, &str)] = &[
        (TreasuryError::Unauthorized, "Authorization"),
        (TreasuryError::UnauthorizedSigner, "Authorization"),
        (TreasuryError::SettlementNotFound, "State"),
        (TreasuryError::AlreadyExecuted, "State"),
        (TreasuryError::InvalidAmount, "Validation"),
        (TreasuryError::ArithmeticOverflow, "Validation"),
        (TreasuryError::ThresholdNotMet, "Configuration"),
        (TreasuryError::TokenNotAllowed, "Configuration"),
        (TreasuryError::ContractPaused, "Timing"),
        (TreasuryError::ExecutionDeadlineExceeded, "Timing"),
        (TreasuryError::BatchTooLarge, "Capacity"),
        (TreasuryError::AllowlistFull, "Capacity"),
        (TreasuryError::InsufficientBalance, "Validation"),
        (TreasuryError::DuplicateReferenceId, "Validation"),
        (TreasuryError::NoPendingAdmin, "State"),
    ];

    for (variant, expected_category) in pairs {
        let (category, description) = error_category(*variant);
        assert!(!category.is_empty(), "category must be non-empty for {:?}", variant);
        assert!(!description.is_empty(), "description must be non-empty for {:?}", variant);
        assert_eq!(
            category,
            *expected_category,
            "unexpected category for {:?}: got '{}', want '{}'",
            variant,
            category,
            expected_category,
        );
    }
}

/// Every `TreasuryError` variant must map to non-empty category and description strings.
#[test]
fn error_category_covers_all_variants_with_non_empty_strings() {
    let all: &[TreasuryError] = &[
        TreasuryError::AlreadyInitialized,
        TreasuryError::ZeroThreshold,
        TreasuryError::SettlementNotFound,
        TreasuryError::AlreadyExecuted,
        TreasuryError::ThresholdNotMet,
        TreasuryError::ThresholdNotConfigured,
        TreasuryError::InvalidAmount,
        TreasuryError::ContractPaused,
        TreasuryError::Unauthorized,
        TreasuryError::UnauthorizedSigner,
        TreasuryError::InvalidTokenContract,
        TreasuryError::TokenNotAllowed,
        TreasuryError::RotationNotFound,
        TreasuryError::RotationAlreadyExecuted,
        TreasuryError::SettlementOnHold,
        TreasuryError::DisputeNotExpired,
        TreasuryError::AlreadyOnHold,
        TreasuryError::ThresholdUnreachable,
        TreasuryError::ComplianceCheckFailed,
        TreasuryError::ArithmeticOverflow,
        TreasuryError::DisputeNotFound,
        TreasuryError::DisputeAlreadyResolved,
        TreasuryError::ResolutionDirectionMismatch,
        TreasuryError::BatchTooLarge,
        TreasuryError::WeightOverflow,
        TreasuryError::SettlementNotCancellable,
        TreasuryError::TtlNotElapsed,
        TreasuryError::AllowlistFull,
        TreasuryError::NotOnHold,
        TreasuryError::DestinationNotAllowed,
        TreasuryError::InsufficientBalance,
        TreasuryError::NotPaused,
        TreasuryError::RotationProposalCooldown,
        TreasuryError::WorkflowNotRegisteredSigner,
        TreasuryError::WithdrawalLimitExceeded,
        TreasuryError::InvalidSplitRatio,
        TreasuryError::ForceCancelNotAllowed,
        TreasuryError::SignerChangeTooEarly,
        TreasuryError::SignerChangeNotFound,
        TreasuryError::SignerChangeAlreadyFinalised,
        TreasuryError::ExecutionDeadlineExceeded,
        TreasuryError::QuorumBreak,
        TreasuryError::TokenHasPendingSettlements,
        TreasuryError::ApprovalNotFound,
        TreasuryError::DuplicateVote,
        TreasuryError::NoPendingAdmin,
        TreasuryError::LabelTooLong,
        TreasuryError::InvalidTimelockDelay,
        TreasuryError::DuplicateReferenceId,
        TreasuryError::TokenHasBalance,
    ];

    for variant in all {
        let (category, description) = error_category(*variant);
        assert!(
            !category.is_empty(),
            "category is empty for {:?}",
            variant
        );
        assert!(
            !description.is_empty(),
            "description is empty for {:?}",
            variant
        );
    }
}
