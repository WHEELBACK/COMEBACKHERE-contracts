// Issue #698: read paths for recurring invoice templates.
use invoice::{
    InvoiceContract, InvoiceContractClient, InvoiceError, MaybeBytes, RecurrenceInterval,
    TEMPLATE_PAGE_LIMIT,
};
use soroban_sdk::{testutils::Address as _, Address, Env};

const AMOUNT: i128 = 25_000_000;
const GROSS: i128 = 25_750_000;

fn setup() -> (Env, Address, Address, InvoiceContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let merchant = Address::generate(&env);
    let id = env.register_contract(None, InvoiceContract);
    let client = InvoiceContractClient::new(&env, &id);
    client.initialize(&admin);
    (env, admin, merchant, client)
}

fn new_template(
    client: &InvoiceContractClient,
    merchant: &Address,
    interval: &RecurrenceInterval,
) -> u64 {
    client.create_template(
        merchant,
        &AMOUNT,
        &GROSS,
        interval,
        &3_600,
        &MaybeBytes::None,
        &MaybeBytes::None,
    )
}

#[test]
fn get_template_returns_the_registered_definition() {
    let (_env, _admin, merchant, client) = setup();
    let id = new_template(&client, &merchant, &RecurrenceInterval::Weekly);

    let template = client.get_invoice_template(&id);
    assert_eq!(template.id, id);
    assert_eq!(template.merchant, merchant);
    assert_eq!(template.amount_usdc, AMOUNT);
    assert_eq!(template.gross_usdc, GROSS);
    assert_eq!(template.interval, RecurrenceInterval::Weekly);
    assert_eq!(template.expires_in_seconds, 3_600);
    assert!(template.active);
}

#[test]
fn get_template_unknown_id_returns_not_found() {
    let (_env, _admin, _merchant, client) = setup();

    let err = client.try_get_invoice_template(&1).unwrap_err().unwrap();
    assert_eq!(err, InvoiceError::NotFound);

    let zero = client.try_get_invoice_template(&0).unwrap_err().unwrap();
    assert_eq!(zero, InvoiceError::NotFound);
}

#[test]
fn list_is_empty_for_a_merchant_without_templates() {
    let (_env, _admin, merchant, client) = setup();

    let listed = client.list_templates_by_merchant(&merchant, &0, &10);
    assert_eq!(listed.len(), 0);
}

#[test]
fn list_returns_a_single_template() {
    let (_env, _admin, merchant, client) = setup();
    let id = new_template(&client, &merchant, &RecurrenceInterval::Daily);

    let listed = client.list_templates_by_merchant(&merchant, &0, &10);
    assert_eq!(listed.len(), 1);
    assert_eq!(listed.get(0).unwrap().id, id);
}

#[test]
fn list_preserves_creation_order() {
    let (_env, _admin, merchant, client) = setup();
    let first = new_template(&client, &merchant, &RecurrenceInterval::Daily);
    let second = new_template(&client, &merchant, &RecurrenceInterval::Weekly);
    let third = new_template(&client, &merchant, &RecurrenceInterval::Monthly);

    let listed = client.list_templates_by_merchant(&merchant, &0, &10);
    let ids: Vec<u64> = (0..listed.len())
        .map(|i| listed.get(i).unwrap().id)
        .collect();
    assert_eq!(ids, [first, second, third]);
}

#[test]
fn list_pages_through_multiple_templates() {
    let (_env, _admin, merchant, client) = setup();
    for _ in 0..5 {
        new_template(&client, &merchant, &RecurrenceInterval::Daily);
    }

    let first_page = client.list_templates_by_merchant(&merchant, &0, &2);
    let second_page = client.list_templates_by_merchant(&merchant, &2, &2);
    let third_page = client.list_templates_by_merchant(&merchant, &4, &2);

    assert_eq!(first_page.len(), 2);
    assert_eq!(second_page.len(), 2);
    assert_eq!(third_page.len(), 1);

    let mut seen: u32 = 0;
    for page in [&first_page, &second_page, &third_page] {
        for i in 0..page.len() {
            assert_eq!(page.get(i).unwrap().id, u64::from(seen) + 1);
            seen += 1;
        }
    }
}

#[test]
fn list_start_past_the_end_returns_an_empty_page() {
    let (_env, _admin, merchant, client) = setup();
    new_template(&client, &merchant, &RecurrenceInterval::Daily);

    let listed = client.list_templates_by_merchant(&merchant, &1, &10);
    assert_eq!(listed.len(), 0);
}

#[test]
fn list_limit_is_capped_at_max_page_limit() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let merchant = Address::generate(&env);
    let id = env.register_contract(None, InvoiceContract);
    let client = InvoiceContractClient::new(&env, &id);
    client.initialize(&admin);

    for _ in 0..(TEMPLATE_PAGE_LIMIT + 5) {
        new_template(&client, &merchant, &RecurrenceInterval::Daily);
    }

    let listed = client.list_templates_by_merchant(&merchant, &0, &1_000);
    assert_eq!(listed.len(), TEMPLATE_PAGE_LIMIT);
}

#[test]
fn list_is_scoped_to_the_requesting_merchant() {
    let (env, _admin, merchant, client) = setup();
    let other_merchant = Address::generate(&env);

    let mine = new_template(&client, &merchant, &RecurrenceInterval::Daily);
    new_template(&client, &other_merchant, &RecurrenceInterval::Daily);

    let listed = client.list_templates_by_merchant(&merchant, &0, &10);
    assert_eq!(listed.len(), 1);
    assert_eq!(listed.get(0).unwrap().id, mine);
}

#[test]
fn templates_survive_pause_and_unpause() {
    let (_env, admin, merchant, client) = setup();
    let id = new_template(&client, &merchant, &RecurrenceInterval::Monthly);

    client.pause(&admin);
    let blocked = client
        .try_create_template(
            &merchant,
            &AMOUNT,
            &GROSS,
            &RecurrenceInterval::Daily,
            &3_600,
            &MaybeBytes::None,
            &MaybeBytes::None,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(blocked, InvoiceError::ContractPaused);

    client.unpause(&admin);
    assert_eq!(client.get_invoice_template(&id).id, id);
}

#[test]
fn template_amount_validation_matches_invoice_rules() {
    let (_env, _admin, merchant, client) = setup();

    let zero_duration = client
        .try_create_template(
            &merchant,
            &AMOUNT,
            &GROSS,
            &RecurrenceInterval::Daily,
            &0,
            &MaybeBytes::None,
            &MaybeBytes::None,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(zero_duration, InvoiceError::ZeroDuration);

    let gross_below_amount = client
        .try_create_template(
            &merchant,
            &GROSS,
            &AMOUNT,
            &RecurrenceInterval::Daily,
            &3_600,
            &MaybeBytes::None,
            &MaybeBytes::None,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(gross_below_amount, InvoiceError::InvalidAmount);

    let sub_usdc = client
        .try_create_template(
            &merchant,
            &1,
            &1,
            &RecurrenceInterval::Daily,
            &3_600,
            &MaybeBytes::None,
            &MaybeBytes::None,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(sub_usdc, InvoiceError::AmountPrecision);
}

#[test]
fn template_activation_is_merchant_scoped_and_reflected_in_reads() {
    let (env, _admin, merchant, client) = setup();
    let id = new_template(&client, &merchant, &RecurrenceInterval::Daily);

    client.set_template_active(&merchant, &id, &false);
    assert!(!client.get_invoice_template(&id).active);

    client.set_template_active(&merchant, &id, &true);
    assert!(client.get_invoice_template(&id).active);

    let stranger = Address::generate(&env);
    let err = client
        .try_set_template_active(&stranger, &id, &false)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, InvoiceError::Unauthorized);
}

#[test]
fn setting_activation_on_an_unknown_template_returns_not_found() {
    let (_env, _admin, merchant, client) = setup();

    let err = client
        .try_set_template_active(&merchant, &404, &false)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, InvoiceError::NotFound);
}

#[test]
fn reading_templates_does_not_touch_the_invoice_ledger() {
    let (_env, _admin, merchant, client) = setup();
    new_template(&client, &merchant, &RecurrenceInterval::Daily);

    // Reads are side-effect free: no invoice exists yet.
    let listed = client.list_templates_by_merchant(&merchant, &0, &10);
    assert_eq!(listed.len(), 1);
    assert!(client.try_get_invoice(&1).is_err());
    assert!(client.try_get_invoice_status(&1).is_err());
}
