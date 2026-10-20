// Issue #699: permissionless keeper that turns due templates into invoices.
use invoice::{
    InvoiceContract, InvoiceContractClient, InvoiceError, InvoiceStatus, MaybeAddress, MaybeBytes,
    RecurrenceInterval, TEMPLATE_KEEPER_BATCH_SIZE,
};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    vec, Address, Env,
};

const AMOUNT: i128 = 40_000_000;
const GROSS: i128 = 41_000_000;
const ONE_DAY: u64 = 86_400;

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
fn template_is_not_due_before_its_first_run() {
    let (env, _admin, merchant, client) = setup();
    let id = new_template(&client, &merchant, &RecurrenceInterval::Daily);

    let generated = client.generate_due_invoices(&vec![&env, id]);
    assert_eq!(generated, 0);
    assert!(client.try_get_invoice(&1).is_err());
}

#[test]
fn due_template_generates_one_invoice_and_advances_the_schedule() {
    let (env, _admin, merchant, client) = setup();
    env.ledger().with_mut(|l| l.timestamp = 1_000);
    let id = new_template(&client, &merchant, &RecurrenceInterval::Daily);
    assert_eq!(
        client.get_invoice_template(&id).next_run_at,
        1_000 + ONE_DAY
    );

    // Move to the first due moment.
    env.ledger().with_mut(|l| l.timestamp = 1_000 + ONE_DAY);
    let generated = client.generate_due_invoices(&vec![&env, id]);
    assert_eq!(generated, 1);

    let invoice = client.get_invoice(&1);
    assert_eq!(invoice.merchant, merchant);
    assert_eq!(invoice.amount_usdc, AMOUNT);
    assert_eq!(invoice.gross_usdc, GROSS);
    assert_eq!(invoice.status, InvoiceStatus::Pending);
    assert_eq!(invoice.expires_at, 1_000 + ONE_DAY + 3_600);

    // The schedule moved past now, so the next period is not due yet.
    assert_eq!(
        client.get_invoice_template(&id).next_run_at,
        1_000 + 2 * ONE_DAY
    );
}

#[test]
fn second_call_in_the_same_period_generates_nothing() {
    let (env, _admin, merchant, client) = setup();
    let id = new_template(&client, &merchant, &RecurrenceInterval::Daily);

    env.ledger().with_mut(|l| l.timestamp = ONE_DAY);
    assert_eq!(client.generate_due_invoices(&vec![&env, id]), 1);
    assert_eq!(client.generate_due_invoices(&vec![&env, id]), 0);
    assert_eq!(client.generate_due_invoices(&vec![&env, id]), 0);

    // Exactly one invoice exists for that period.
    assert_eq!(client.get_invoice(&1).id, 1);
    assert!(client.try_get_invoice(&2).is_err());
}

#[test]
fn repeated_periods_generate_one_invoice_each() {
    let (env, _admin, merchant, client) = setup();
    let id = new_template(&client, &merchant, &RecurrenceInterval::Daily);

    for day in 1..=3u64 {
        env.ledger().with_mut(|l| l.timestamp = day * ONE_DAY);
        assert_eq!(
            client.generate_due_invoices(&vec![&env, id]),
            1,
            "day {day} should generate exactly one invoice"
        );
    }

    assert_eq!(client.get_invoice(&3).id, 3);
    assert!(client.try_get_invoice(&4).is_err());
}

#[test]
fn one_call_after_a_long_gap_generates_a_single_invoice() {
    let (env, _admin, merchant, client) = setup();
    let id = new_template(&client, &merchant, &RecurrenceInterval::Daily);

    // Five periods elapsed in one step.
    env.ledger().with_mut(|l| l.timestamp = 5 * ONE_DAY);
    let generated = client.generate_due_invoices(&vec![&env, id]);

    // No backfill: one invoice now, and the schedule resumes from the future.
    assert_eq!(generated, 1);
    assert_eq!(client.get_invoice_template(&id).next_run_at, 6 * ONE_DAY);
    assert!(client.try_get_invoice(&2).is_err());
}

#[test]
fn batch_generates_for_every_due_template() {
    let (env, _admin, merchant, client) = setup();
    let daily = new_template(&client, &merchant, &RecurrenceInterval::Daily);
    let weekly = new_template(&client, &merchant, &RecurrenceInterval::Weekly);
    let not_due = new_template(&client, &merchant, &RecurrenceInterval::Monthly);

    // A week after creation the daily and weekly templates are due; the
    // monthly one is not.
    env.ledger().with_mut(|l| l.timestamp = 604_800);
    let generated = client.generate_due_invoices(&vec![&env, daily, weekly, not_due]);

    assert_eq!(generated, 2);
    assert_eq!(client.get_invoice(&1).id, 1);
    assert_eq!(client.get_invoice(&2).id, 2);
    assert!(client.try_get_invoice(&3).is_err());
    assert_eq!(client.get_invoice_template(&not_due).next_run_at, 2_592_000);
}

#[test]
fn unknown_template_ids_are_skipped() {
    let (env, _admin, merchant, client) = setup();
    let id = new_template(&client, &merchant, &RecurrenceInterval::Daily);

    env.ledger().with_mut(|l| l.timestamp = ONE_DAY);
    let generated = client.generate_due_invoices(&vec![&env, 9_999, id, 12_345]);

    assert_eq!(generated, 1);
}

#[test]
fn deactivated_template_is_not_generated() {
    let (env, _admin, merchant, client) = setup();
    let id = new_template(&client, &merchant, &RecurrenceInterval::Daily);
    client.set_template_active(&merchant, &id, &false);

    env.ledger().with_mut(|l| l.timestamp = ONE_DAY);
    assert_eq!(client.generate_due_invoices(&vec![&env, id]), 0);

    // Reactivation keeps the original schedule; the period is still billable.
    client.set_template_active(&merchant, &id, &true);
    assert_eq!(client.generate_due_invoices(&vec![&env, id]), 1);
}

#[test]
fn batch_larger_than_max_batch_size_is_rejected() {
    let (env, _admin, _merchant, client) = setup();
    let mut ids: soroban_sdk::Vec<u64> = soroban_sdk::vec![&env];
    for id in 1..=u64::from(TEMPLATE_KEEPER_BATCH_SIZE) + 1 {
        ids.push_back(id);
    }

    let err = client.try_generate_due_invoices(&ids).unwrap_err().unwrap();
    assert_eq!(err, InvoiceError::BatchTooLarge);
}

#[test]
fn batch_at_max_batch_size_is_accepted() {
    let (env, _admin, merchant, client) = setup();
    let mut ids: soroban_sdk::Vec<u64> = soroban_sdk::vec![&env];
    for _ in 0..TEMPLATE_KEEPER_BATCH_SIZE {
        let id = new_template(&client, &merchant, &RecurrenceInterval::Daily);
        ids.push_back(id);
    }

    env.ledger().with_mut(|l| l.timestamp = ONE_DAY);
    assert_eq!(client.generate_due_invoices(&ids), TEMPLATE_KEEPER_BATCH_SIZE);
}

#[test]
fn empty_batch_is_a_no_op() {
    let (env, _admin, _merchant, client) = setup();

    assert_eq!(client.generate_due_invoices(&vec![&env]), 0);
}

#[test]
fn keeper_is_blocked_while_paused() {
    let (env, admin, merchant, client) = setup();
    let id = new_template(&client, &merchant, &RecurrenceInterval::Daily);

    env.ledger().with_mut(|l| l.timestamp = ONE_DAY);
    client.pause(&admin);

    let err = client
        .try_generate_due_invoices(&vec![&env, id])
        .unwrap_err()
        .unwrap();
    assert_eq!(err, InvoiceError::ContractPaused);
}

#[test]
fn generated_invoices_are_ordinary_invoices() {
    let (env, admin, merchant, client) = setup();
    let id = new_template(&client, &merchant, &RecurrenceInterval::Daily);

    env.ledger().with_mut(|l| l.timestamp = ONE_DAY);
    client.generate_due_invoices(&vec![&env, id]);

    let payer = Address::generate(&env);
    client.mark_paid(&admin, &1, &payer, &MaybeBytes::None, &MaybeAddress::None);
    client.release_escrow(&admin, &1);

    let invoice = client.get_invoice(&1);
    assert_eq!(invoice.status, InvoiceStatus::Released);
    assert_eq!(invoice.merchant, merchant);
}
