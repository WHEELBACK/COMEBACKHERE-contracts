//! #711: `get_invoices_by_payer` - the payer-side counterpart to the
//! per-merchant index.
//!
//! A wallet can list a merchant's invoices but had no way to list the invoices
//! addressed to a payer, forcing a full scan to answer "what do I owe?". These
//! tests cover the paging contract itself (offsets, the `MAX_PAGE_LIMIT` clamp,
//! past-the-end offsets), the invariant that only invoices with a designated
//! payer are indexed, and the documented `MAX_PAYER_INDEX_ENTRIES` growth cap -
//! reaching the cap must slow the index down, never block a payment.

use invoice::{
    InvoiceContract, InvoiceContractClient, InvoiceStatus, MaybeAddress, MaybeBytes,
    MAX_PAGE_LIMIT, MAX_PAYER_INDEX_ENTRIES,
};
use soroban_sdk::{testutils::Address as _, Address, Env, Vec};

const AMOUNT_USDC: i128 = 10_000_000;
const GROSS_USDC: i128 = 10_250_000;
const EXPIRES_IN: u64 = 3_600;

struct Fixture {
    env: Env,
    admin: Address,
    merchant: Address,
    client: InvoiceContractClient<'static>,
}

fn setup() -> Fixture {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let merchant = Address::generate(&env);
    let contract_id = env.register_contract(None, InvoiceContract);
    let client = InvoiceContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    Fixture {
        env,
        admin,
        merchant,
        client,
    }
}

fn create(f: &Fixture) -> u64 {
    f.client.create_invoice(
        &f.merchant,
        &AMOUNT_USDC,
        &GROSS_USDC,
        &EXPIRES_IN,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
        &MaybeAddress::None,
    )
}

/// Create an invoice and mark it paid by `payer`, returning the invoice id.
fn create_and_pay(f: &Fixture, payer: &Address) -> u64 {
    let id = create(f);
    f.client
        .mark_paid(&f.admin, &id, payer, &MaybeBytes::None, &MaybeAddress::None);
    id
}

#[test]
fn payer_index_returns_every_invoice_that_payer_paid() {
    let f = setup();
    let payer = Address::generate(&f.env);
    let first = create_and_pay(&f, &payer);
    let second = create_and_pay(&f, &payer);

    let page = f.client.get_invoices_by_payer(&payer, &0, &10);

    assert_eq!(page, Vec::from_array(&f.env, [first, second]));
}

#[test]
fn payer_index_is_scoped_to_one_payer() {
    let f = setup();
    let payer_a = Address::generate(&f.env);
    let payer_b = Address::generate(&f.env);
    let a1 = create_and_pay(&f, &payer_a);
    let _b1 = create_and_pay(&f, &payer_b);
    let a2 = create_and_pay(&f, &payer_a);

    assert_eq!(
        f.client.get_invoices_by_payer(&payer_a, &0, &10),
        Vec::from_array(&f.env, [a1, a2])
    );
    let b_page = f.client.get_invoices_by_payer(&payer_b, &0, &10);
    assert_eq!(b_page.len(), 1);
    assert_eq!(
        f.client.get_invoices_by_payer(&payer_a, &0, &10),
        Vec::from_array(&f.env, [a1, a2]),
        "payer A's page must not include payer B's invoice"
    );
}

#[test]
fn payer_index_preserves_payment_order_across_pagination() {
    let f = setup();
    let payer = Address::generate(&f.env);
    let mut ids: Vec<u64> = Vec::new(&f.env);
    for _ in 0..5 {
        ids.push_back(create_and_pay(&f, &payer));
    }

    let mut paged = Vec::new(&f.env);
    let mut start = 0u32;
    loop {
        let page = f.client.get_invoices_by_payer(&payer, &start, &2);
        if page.is_empty() {
            break;
        }
        for id in page.iter() {
            paged.push_back(id);
        }
        start += 2;
    }

    assert_eq!(
        paged, ids,
        "paging must reproduce the payment order exactly"
    );
    assert_eq!(
        f.client.get_invoices_by_payer(&payer, &0, &2),
        Vec::from_array(&f.env, [ids.get(0).unwrap(), ids.get(1).unwrap()])
    );
    assert_eq!(
        f.client.get_invoices_by_payer(&payer, &4, &2),
        Vec::from_array(&f.env, [ids.get(4).unwrap()])
    );
}

#[test]
fn offset_past_the_end_returns_an_empty_page() {
    let f = setup();
    let payer = Address::generate(&f.env);
    create_and_pay(&f, &payer);

    assert!(f.client.get_invoices_by_payer(&payer, &1, &10).is_empty());
    assert!(f
        .client
        .get_invoices_by_payer(&payer, &u32::MAX, &10)
        .is_empty());
    assert!(f.client.get_invoices_by_payer(&payer, &0, &0).is_empty());
}

#[test]
fn oversized_limit_is_clamped_to_the_maximum_page() {
    let f = setup();
    let payer = Address::generate(&f.env);
    let total = MAX_PAGE_LIMIT + 5;
    let mut expected_len = 0u32;
    for i in 0..total {
        create_and_pay(&f, &payer);
        expected_len = expected_len.max(i + 1);
    }

    let page = f.client.get_invoices_by_payer(&payer, &0, &u32::MAX);

    assert_eq!(page.len(), MAX_PAGE_LIMIT);
    assert!(expected_len > MAX_PAGE_LIMIT);
}

#[test]
fn invoices_without_a_designated_payer_are_not_indexed() {
    let f = setup();
    let payer = Address::generate(&f.env);
    let other = Address::generate(&f.env);
    let unpaid = create(&f);
    create_and_pay(&f, &payer);

    let page = f.client.get_invoices_by_payer(&payer, &0, &10);

    assert_eq!(page.len(), 1);
    assert!(!page.contains(unpaid), "a Pending invoice has no payer yet");
    // Unpaid invoices are reachable through the global page instead.
    assert_eq!(f.client.get_invoice_status(&unpaid), InvoiceStatus::Pending);
    assert!(f.client.get_invoices_by_payer(&other, &0, &10).is_empty());
}

#[test]
fn a_payer_who_paid_nothing_has_an_empty_index() {
    let f = setup();
    let stranger = Address::generate(&f.env);
    create_and_pay(&f, &Address::generate(&f.env));

    assert!(f
        .client
        .get_invoices_by_payer(&stranger, &0, &10)
        .is_empty());
}

#[test]
fn the_index_is_updated_when_an_invoice_is_paid_not_when_it_is_created() {
    let f = setup();
    let payer = Address::generate(&f.env);
    let first = create(&f);
    let second = create(&f);

    // Creation carries no payer, so nothing is indexed yet.
    assert!(f.client.get_invoices_by_payer(&payer, &0, &10).is_empty());

    f.client.mark_paid(
        &f.admin,
        &first,
        &payer,
        &MaybeBytes::None,
        &MaybeAddress::None,
    );

    assert_eq!(
        f.client.get_invoices_by_payer(&payer, &0, &10),
        Vec::from_array(&f.env, [first]),
        "the first payment adds its invoice"
    );

    f.client.mark_paid(
        &f.admin,
        &second,
        &payer,
        &MaybeBytes::None,
        &MaybeAddress::None,
    );

    assert_eq!(
        f.client.get_invoices_by_payer(&payer, &0, &10),
        Vec::from_array(&f.env, [first, second]),
        "the second payment appends, never reorders"
    );
}

#[test]
fn refund_flow_leaves_the_payer_index_unchanged() {
    let f = setup();
    let payer = Address::generate(&f.env);
    let refunded = create_and_pay(&f, &payer);
    let paid = create_and_pay(&f, &payer);

    f.client.request_refund(&payer, &refunded);
    f.client.approve_refund(&f.admin, &refunded);
    f.client.request_refund(&payer, &paid);
    f.client.reject_refund(&f.admin, &paid);

    let page = f.client.get_invoices_by_payer(&payer, &0, &10);

    // Neither a refund nor a rejection reassigns the payer, so the index needs
    // no migration and keeps pointing at the same invoice.
    assert_eq!(page, Vec::from_array(&f.env, [refunded, paid]));
}

#[test]
fn index_growth_is_capped_without_blocking_payments() {
    let f = setup();
    let payer = Address::generate(&f.env);

    // Fill the index exactly to its cap, then overshoot by one.
    for _ in 0..MAX_PAYER_INDEX_ENTRIES {
        create_and_pay(&f, &payer);
    }
    let overflow_id = create_and_pay(&f, &payer);

    assert_eq!(
        f.client.get_invoice_status(&overflow_id),
        InvoiceStatus::Paid,
        "the payment beyond the cap must still succeed"
    );

    // Every indexed entry is reachable, and the overflow invoice is not indexed.
    let mut seen = 0u64;
    let mut start = 0u32;
    loop {
        let page = f
            .client
            .get_invoices_by_payer(&payer, &start, &MAX_PAGE_LIMIT);
        if page.is_empty() {
            break;
        }
        assert!(!page.contains(overflow_id));
        seen += page.len() as u64;
        start += MAX_PAGE_LIMIT;
    }
    assert_eq!(seen, MAX_PAYER_INDEX_ENTRIES);
}
