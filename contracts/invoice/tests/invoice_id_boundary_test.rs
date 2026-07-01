use invoice::{DataKey, InvoiceContract, InvoiceContractClient, MaybeAddress, MaybeBytes};
use soroban_sdk::{testutils::Address as _, Address, Env};

fn setup() -> (Env, Address, Address, InvoiceContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let contract_id = env.register_contract(None, InvoiceContract);
    let client = InvoiceContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    (env, admin, contract_id, client)
}

// ID 0 (u64::MIN) is never assigned — lookup must panic (unwrap on missing key).
#[test]
#[should_panic]
fn test_get_invoice_id_zero_panics() {
    let (_, _, _, client) = setup();
    client.get_invoice(&0u64);
}

// u64::MAX is never assigned; lookup must panic.
#[test]
#[should_panic]
fn test_get_invoice_id_u64_max_panics() {
    let (_, _, _, client) = setup();
    client.get_invoice(&u64::MAX);
}

// u64::MAX - 1 is also never assigned by default; lookup must panic.
#[test]
#[should_panic]
fn test_get_invoice_id_u64_max_minus_one_panics() {
    let (_, _, _, client) = setup();
    client.get_invoice(&(u64::MAX - 1));
}

// Invoice IDs start at 1 and increment sequentially — u64::MIN + 1 is the first ID.
#[test]
fn test_first_invoice_id_is_one() {
    let (env, _, _, client) = setup();
    let merchant = Address::generate(&env);
    let id = client.create_invoice(
        &merchant,
        &10_000_000,
        &10_000_000,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
        &MaybeAddress::None,
    );
    assert_eq!(id, 1u64);
}

// Sequential IDs increment without gaps across many creations.
#[test]
fn test_sequential_ids_increment_correctly() {
    let (env, _, _, client) = setup();
    let merchant = Address::generate(&env);
    for expected in 1u64..=10 {
        let id = client.create_invoice(
            &merchant,
            &10_000_000,
            &10_000_000,
            &3600,
            &MaybeBytes::None,
            &MaybeBytes::None,
            &0,
            &MaybeAddress::None,
        );
        assert_eq!(id, expected);
    }
}

// Invoice at u64::MAX - 1: seed the counter to MAX-2 via as_contract, then
// create one invoice to land at MAX-1 and verify retrieval is correct.
#[test]
fn test_invoice_at_large_boundary_id_retrievable() {
    let (env, _, contract_id, client) = setup();
    let env2 = env.clone();
    env.as_contract(&contract_id, || {
        env2.storage()
            .instance()
            .set(&DataKey::InvoiceCount, &(u64::MAX - 2));
    });
    let merchant = Address::generate(&env);
    let id = client.create_invoice(
        &merchant,
        &10_000_000,
        &11_000_000,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
        &MaybeAddress::None,
    );
    assert_eq!(id, u64::MAX - 1);
    let inv = client.get_invoice(&id);
    assert_eq!(inv.id, u64::MAX - 1);
    assert_eq!(inv.amount_usdc, 10_000_000);
    assert_eq!(inv.gross_usdc, 11_000_000);
}

// Invoice at u64::MAX: seed counter to MAX-1, create one invoice at MAX,
// then verify the next creation overflows (no silent wrap to 0).
#[test]
fn test_overflow_wrapping_at_u64_max_is_not_silent() {
    let (env, _, contract_id, client) = setup();
    let env2 = env.clone();
    env.as_contract(&contract_id, || {
        env2.storage()
            .instance()
            .set(&DataKey::InvoiceCount, &(u64::MAX - 1));
    });
    let merchant = Address::generate(&env);

    // First creation: counter was MAX-1, new id = MAX — should succeed.
    let id = client.create_invoice(
        &merchant,
        &10_000_000,
        &10_000_000,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
        &MaybeAddress::None,
    );
    assert_eq!(id, u64::MAX);

    // Second creation: counter is now MAX, new id = MAX + 1 — must not silently
    // produce 0; the arithmetic overflow should be detected (panic in debug, or
    // wrapping to 0 which we also reject as a regression guard).
    let result = client.try_create_invoice(
        &merchant,
        &10_000_000,
        &10_000_000,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
        &MaybeAddress::None,
    );
    if let Ok(Ok(wrapped_id)) = result {
        // If the runtime wraps instead of panicking, the ID must not be 0 —
        // a 0 ID would collide with the "no invoice" sentinel.
        assert_ne!(
            wrapped_id, 0,
            "overflow silently produced id=0 (collides with missing-invoice sentinel)"
        );
    }
    // Err result (host panic / contract error) is also acceptable.
}
