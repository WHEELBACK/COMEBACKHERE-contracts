//! Fails to compile (and therefore fails CI) if `ComplianceClient`'s expected
//! signatures drift from the compiled compliance contract's public interface.
//! This is the seam treasury relies on for cross-contract calls into compliance.

use compliance::ComplianceContract;
use compliance_client::ComplianceClient;
use soroban_sdk::{testutils::Address as _, Address, Env};

#[test]
fn compliance_client_matches_compliance_contract_abi() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let subject = Address::generate(&env);

    let contract_id = env.register_contract(None, ComplianceContract);
    let client = ComplianceClient::new(&env, &contract_id);

    // Each call below type-checks `ComplianceClient`'s method signatures
    // against the actual compiled `ComplianceContract` interface. If the
    // compliance contract's public API changes shape, this file fails to
    // compile, catching the drift before treasury does at runtime.
    client.initialize(&admin);
    client.allow_address(&admin, &subject);
    assert!(client.is_allowed(&subject));

    let addresses = soroban_sdk::vec![&env, subject.clone()];
    let results = client.bulk_check_addresses(&addresses);
    assert_eq!(results.len(), 1);

    let page = client.export_snapshot_page(&admin, &0u64, &10u64);
    assert_eq!(page.len(), 1);

    client.block_address(&admin, &subject, &None);
    assert!(!client.is_allowed(&subject));
}
