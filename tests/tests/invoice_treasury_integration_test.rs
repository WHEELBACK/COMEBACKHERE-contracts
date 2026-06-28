use invoice::{
    InvoiceContract, InvoiceContractClient, InvoiceError, InvoiceStatus, MaybeAddress, MaybeBytes,
};
use soroban_sdk::{contract, contractimpl, testutils::Address as _, Address, Env};
use treasury::{TreasuryContract, TreasuryContractClient};

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

fn setup() -> (
    Env,
    Address,
    Address,
    Address,
    InvoiceContractClient<'static>,
    Address,
    TreasuryContractClient<'static>,
    Address,
) {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let merchant = Address::generate(&env);
    let payer = Address::generate(&env);

    let invoice_id = env.register_contract(None, InvoiceContract);
    let invoice = InvoiceContractClient::new(&env, &invoice_id);
    invoice.initialize(&admin);

    let treasury_id = env.register_contract(None, TreasuryContract);
    let treasury = TreasuryContractClient::new(&env, &treasury_id);
    treasury.initialize(&admin, &1);

    let token_id = env.register_contract(None, TestToken);
    (env, admin, merchant, payer, invoice, treasury_id, treasury, token_id)
}

#[test]
fn invoice_created_paid_released() {
    let (env, admin, merchant, payer, invoice, _treasury_id, _treasury, _token_id) = setup();

    let id = invoice.create_invoice(
        &merchant,
        &10_000_000,
        &10_250_000,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
    );
    let inv = invoice.get_invoice(&id);
    assert_eq!(inv.status, InvoiceStatus::Pending);

    invoice.mark_paid(&admin, &id, &payer);
    let inv = invoice.get_invoice(&id);
    assert_eq!(inv.status, InvoiceStatus::Paid);
    assert_eq!(inv.payer, MaybeAddress::Some(payer.clone()));

    invoice.release_escrow(&admin, &id);
    let inv = invoice.get_invoice(&id);
    assert_eq!(inv.status, InvoiceStatus::Released);
}

#[test]
fn treasury_settlement_after_invoice_release() {
    let (env, admin, merchant, _payer, _invoice, treasury_id, treasury, token_id) = setup();

    let settlement_id = treasury.propose_settlement(&admin, &merchant, &10_000_000);
    let settlement = treasury.get_settlement(&settlement_id);
    assert_eq!(settlement.merchant_address, merchant);
    assert_eq!(settlement.amount, 10_000_000);
    assert_eq!(settlement.approval_weight, 1);

    let token = TestTokenClient::new(&env, &token_id);
    token.mint(&treasury_id, &10_000_000);

    treasury.execute_settlement(&admin, &settlement_id, &token_id);
    let settled = treasury.get_settlement(&settlement_id);
    assert_eq!(settled.status, treasury::SettlementStatus::Executed);
    assert_eq!(token.balance(&merchant), 10_000_000);
}

#[test]
fn end_to_end_invoice_to_settlement() {
    let (env, admin, merchant, payer, invoice, treasury_id, treasury, token_id) = setup();

    let inv_id = invoice.create_invoice(
        &merchant,
        &10_000_000,
        &10_250_000,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
    );

    invoice.mark_paid(&admin, &inv_id, &payer);
    assert_eq!(invoice.get_invoice(&inv_id).status, InvoiceStatus::Paid);

    invoice.release_escrow(&admin, &inv_id);
    assert_eq!(
        invoice.get_invoice(&inv_id).status,
        InvoiceStatus::Released
    );

    let token = TestTokenClient::new(&env, &token_id);
    token.mint(&treasury_id, &10_000_000);
    assert_eq!(token.balance(&treasury_id), 10_000_000);
    assert_eq!(token.balance(&merchant), 0);

    let settlement_id = treasury.propose_settlement(&admin, &merchant, &10_000_000);
    treasury.execute_settlement(&admin, &settlement_id, &token_id);

    assert_eq!(token.balance(&treasury_id), 0);
    assert_eq!(token.balance(&merchant), 10_000_000);
}
