//! Fails to compile (and therefore fails CI) if `ComplianceClient`'s `is_allowed`
//! signature drifts from the compiled compliance contract's public interface.
//! This is the seam treasury and settlement-workflow rely on for cross-contract
//! calls into compliance.
//!
//! `ComplianceClient` deliberately exposes only `is_allowed` (see Cargo.toml and
//! src/lib.rs): it is a `#[contractclient]`-generated invocation client, not a
//! dependency on the `comebackhere-compliance` implementation crate, so that
//! contracts depending on this crate don't statically link compliance's own
//! wasm exports (`pause`, `unpause`, ...) and collide with their own. The real
//! generated client is used below only to drive compliance contract state.

use compliance::{ComplianceContract, ComplianceContractClient};
use compliance_client::ComplianceClient;
use soroban_sdk::{testutils::Address as _, Address, Env};

#[derive(Debug, Eq, PartialEq)]
enum TestError {
    Unauthorized,
}

#[test]
fn wrapper_matches_generated_client_compliance_checks() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let subject = Address::generate(&env);
    let contract_id = env.register_contract(None, ComplianceContract);
    let generated = ComplianceContractClient::new(&env, &contract_id);
    let wrapped = ComplianceClient::new(&env, &contract_id);
    generated.initialize(&admin);

    // Each call below type-checks `ComplianceClient::is_allowed`'s signature
    // against the actual compiled `ComplianceContract` interface. If
    // compliance's `is_allowed` shape changes, this file fails to compile,
    // catching the drift before treasury/settlement-workflow do at runtime.
    assert_eq!(wrapped.is_allowed(&subject), generated.is_allowed(&subject));
    assert_eq!(
        wrapped.require_allowed(&subject, TestError::Unauthorized),
        Err(TestError::Unauthorized)
    );

    generated.allow_address(&admin, &subject);
    assert_eq!(wrapped.is_allowed(&subject), generated.is_allowed(&subject));
    assert_eq!(
        wrapped.require_allowed(&subject, TestError::Unauthorized),
        Ok(())
    );

    generated.block_address(&admin, &subject, &None);
    assert_eq!(wrapped.is_allowed(&subject), generated.is_allowed(&subject));
    assert!(!wrapped.is_allowed(&subject));
}
