//! #872: fuzzing suite for `settlement-workflow` covering malicious callers.
//!
//! A deterministic, seeded fuzz loop that runs under plain `cargo test` (the
//! cargo-fuzz counterpart lives in `fuzz/fuzz_targets/malicious_caller.rs`).
//! Every iteration picks an attacker — a random outsider, a stale former
//! nominee, or one of the pinned protocol contracts themselves — and throws a
//! privileged or value-moving entrypoint at the workflow with random
//! arguments. Auth is mocked for everyone, so the only thing standing between
//! the attacker and the contract is the workflow's own stored-admin checks.
//!
//! After every call the suite asserts that nothing an attacker can reach has
//! moved: the admin is unchanged, the workflow is not paused, no emergency
//! pause is recorded, no settlement executed, and `verify_invariants` (#873)
//! still holds.

use comebackhere_tests::fixtures::{setup_with_workflow, WorkflowFixture};
use settlement_workflow::WorkflowError;
use soroban_sdk::{contract, contractimpl, testutils::Address as _, vec, Address, Env, Vec};

const ITERATIONS: u32 = 256;
const SEEDS: [u64; 4] = [0x9E37_79B9_7F4A_7C15, 1, 0xDEAD_BEEF, u64::MAX];

#[contract]
struct InertTarget;

#[contractimpl]
impl InertTarget {
    pub fn pause(_env: Env, _admin: Address) {}
    pub fn unpause(_env: Env, _admin: Address) {}
}

/// xorshift64* — dependency-free, reproducible from the seed.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed | 1)
    }
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn settlement_id(&mut self) -> u64 {
        match self.below(4) {
            0 => 0,
            1 => u64::MAX,
            _ => self.next(),
        }
    }
}

struct Harness {
    env: Env,
    f: WorkflowFixture,
    token: Address,
    attackers: std::vec::Vec<Address>,
}

fn harness() -> Harness {
    let env = Env::default();
    let f = setup_with_workflow(&env, true);
    let token = env.register_stellar_asset_contract(f.admin.clone());

    // The real admin configures the emergency pause, so attackers face a
    // fully-configured contract rather than a `NotConfigured` short-circuit.
    let target = env.register(InertTarget, ());
    f.workflow
        .initialize_emergency_pause(&f.admin, &vec![&env, target]);

    let mut attackers = std::vec::Vec::new();
    for _ in 0..4 {
        attackers.push(Address::generate(&env));
    }
    // Contracts the workflow trusts must not inherit its authority either.
    attackers.push(f.compliance_contract_id.clone());
    attackers.push(f.treasury_contract_id.clone());
    attackers.push(f.workflow_contract_id.clone());
    attackers.push(token.clone());

    Harness {
        env,
        f,
        token,
        attackers,
    }
}

fn assert_untouched(h: &Harness, iteration: u32, action: u64) {
    let ctx = || format!("iteration {iteration}, action {action}");
    assert_eq!(
        h.f.workflow.get_admin(),
        h.f.admin,
        "admin moved: {}",
        ctx()
    );
    assert!(!h.f.workflow.is_paused(), "workflow paused: {}", ctx());
    assert_eq!(
        h.f.workflow.get_emergency_paused_at(),
        None,
        "emergency pause recorded: {}",
        ctx()
    );
    assert_eq!(
        h.f.workflow.get_emergency_pause_admin(),
        h.f.admin,
        "pause admin moved: {}",
        ctx()
    );
    assert_eq!(
        h.f.workflow.try_verify_invariants(),
        Ok(Ok(())),
        "invariant broken: {}",
        ctx()
    );
}

fn run(seed: u64) {
    let h = harness();
    let mut rng = Rng::new(seed);

    for i in 0..ITERATIONS {
        let attacker = h.attackers[rng.below(h.attackers.len() as u64) as usize].clone();
        let action = rng.below(9);
        match action {
            0 => assert!(h.f.workflow.try_pause(&attacker).is_err()),
            1 => assert!(h.f.workflow.try_unpause(&attacker).is_err()),
            2 => {
                let nominee = h.attackers[rng.below(h.attackers.len() as u64) as usize].clone();
                assert!(h
                    .f
                    .workflow
                    .try_transfer_admin(&attacker, &nominee)
                    .is_err());
            }
            3 => assert!(h.f.workflow.try_accept_admin(&attacker).is_err()),
            4 => assert_eq!(
                h.f.workflow.try_emergency_pause_all(&attacker),
                Err(Ok(WorkflowError::Unauthorized))
            ),
            5 => assert_eq!(
                h.f.workflow.try_resume_all(&attacker, &rng.next()),
                Err(Ok(WorkflowError::Unauthorized))
            ),
            6 => {
                // Attempting to steal the emergency-pause role with an
                // otherwise valid target set.
                let targets: Vec<Address> = vec![&h.env, h.token.clone()];
                assert_eq!(
                    h.f.workflow
                        .try_initialize_emergency_pause(&attacker, &targets),
                    Err(Ok(WorkflowError::Unauthorized))
                );
            }
            7 => assert!(h
                .f
                .workflow
                .try_execute_with_compliance(&rng.settlement_id(), &h.token, &attacker)
                .is_err()),
            _ => {
                let mut ids: Vec<u64> = Vec::new(&h.env);
                for _ in 0..rng.below(6) {
                    ids.push_back(rng.settlement_id());
                }
                match h
                    .f
                    .workflow
                    .try_execute_with_compliance_batch(&ids, &h.token, &attacker)
                {
                    Ok(Ok(executed)) => assert!(executed.is_empty()),
                    Ok(Err(_)) | Err(_) => {}
                }
            }
        }
        assert_untouched(&h, i, action);
    }
}

#[test]
fn malicious_callers_cannot_move_workflow_state() {
    for seed in SEEDS {
        run(seed);
    }
}

/// A former nominee whose nomination was superseded must not be able to
/// accept, no matter how many times the admin re-nominates.
#[test]
fn superseded_nominees_cannot_accept() {
    let h = harness();
    let mut rng = Rng::new(42);
    let mut stale: std::vec::Vec<Address> = std::vec::Vec::new();
    let mut current = Address::generate(&h.env);
    h.f.workflow.transfer_admin(&h.f.admin, &current);

    for _ in 0..32 {
        stale.push(current.clone());
        current = Address::generate(&h.env);
        h.f.workflow.transfer_admin(&h.f.admin, &current);
        let pick = stale[rng.below(stale.len() as u64) as usize].clone();
        assert!(h.f.workflow.try_accept_admin(&pick).is_err());
        assert_eq!(h.f.workflow.get_admin(), h.f.admin);
    }
    h.f.workflow.accept_admin(&current);
    assert_eq!(h.f.workflow.get_admin(), current);
    assert_eq!(h.f.workflow.try_verify_invariants(), Ok(Ok(())));
}
