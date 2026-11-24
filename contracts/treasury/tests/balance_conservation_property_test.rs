use proptest::prelude::*;
use soroban_sdk::{testutils::Address as _, token, Address, Env};
use treasury::{TreasuryContract, TreasuryContractClient};

#[derive(Clone, Debug)]
enum Op {
    Deposit(u8, i128),
    BatchDeposit(u8, i128, i128),
    Withdraw(u8, i128),
    PartialSettle(u8, i128),
    Hold(u8, i128),
    ReleaseHold(u8, i128),
}

fn op_strategy() -> impl Strategy<Value = Op> {
    prop_oneof![
        (0u8..4, 1i128..1_000_000).prop_map(|(who, amt)| Op::Deposit(who, amt)),
        (0u8..4, 1i128..500_000, 1i128..500_000)
            .prop_map(|(who, a, b)| Op::BatchDeposit(who, a, b)),
        (0u8..4, 1i128..1_000_000).prop_map(|(who, amt)| Op::Withdraw(who, amt)),
        (0u8..4, 1i128..1_000_000).prop_map(|(who, amt)| Op::PartialSettle(who, amt)),
        (0u8..4, 1i128..1_000_000).prop_map(|(who, amt)| Op::Hold(who, amt)),
        (0u8..4, 1i128..1_000_000).prop_map(|(who, amt)| Op::ReleaseHold(who, amt)),
    ]
}

/// Sum of every account's tracked balance plus every outstanding hold for `token_id`.
fn tracked_balance_plus_holds(
    client: &TreasuryContractClient,
    users: &[Address],
    token_id: &Address,
) -> i128 {
    users
        .iter()
        .map(|u| client.get_balance(u, token_id) + client.get_hold(u, token_id))
        .sum()
}

proptest! {
    // Conservation invariant: the treasury's on-chain token balance always equals the sum
    // of every account's tracked `Balance` entry, across any sequence of deposit,
    // batch_deposit and withdraw operations (settlement execution is covered by the
    // multisig-specific settlement tests; this property focuses on the deposit/withdraw
    // bookkeeping path shared by all of them).
    #![proptest_config(ProptestConfig::with_cases(256))]
    #[test]
    fn total_deposits_minus_withdrawals_equals_tracked_balance(ops in prop::collection::vec(op_strategy(), 1..30)) {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let contract_id = env.register_contract(None, TreasuryContract);
        let client = TreasuryContractClient::new(&env, &contract_id);
        client.initialize(&admin, &1, &soroban_sdk::Vec::new(&env));

        let token_id = env.register_stellar_asset_contract(admin.clone());
        let asset_client = token::StellarAssetClient::new(&env, &token_id);
        let token_client = token::Client::new(&env, &token_id);

        // Fixed set of participant addresses, each pre-funded generously so deposits never
        // fail for lack of funds.
        let users: Vec<Address> = (0..4).map(|_| Address::generate(&env)).collect();
        for u in &users {
            asset_client.mint(u, &10_000_000_000);
        }

        let mut tracked_total: i128 = 0;

        for op in ops {
            match op {
                Op::Deposit(who, amt) => {
                    let user = &users[(who % 4) as usize];
                    client.deposit(user, &token_id, &amt, &None);
                    tracked_total += amt;
                }
                Op::BatchDeposit(who, a, b) => {
                    let user = &users[(who % 4) as usize];
                    let mut deposits = soroban_sdk::Vec::new(&env);
                    deposits.push_back((token_id.clone(), a));
                    deposits.push_back((token_id.clone(), b));
                    client.batch_deposit(user, &deposits, &None);
                    tracked_total += a + b;
                }
                Op::Withdraw(who, amt) => {
                    let user = &users[(who % 4) as usize];
                    let current: i128 = client.get_balance(user, &token_id);
                    if amt <= current {
                        client.withdraw(user, &token_id, &amt);
                        tracked_total -= amt;
                    }
                    // Withdrawals exceeding the caller's tracked balance would panic
                    // (InsufficientBalance); we skip issuing them so the sequence keeps
                    // generating further operations, consistent with how batch helpers in
                    // this codebase skip invalid entries rather than aborting.
                }
            }
        }

        let contract_token_balance = token_client.balance(&contract_id);
        let tracked_sum: i128 = users.iter().map(|u| client.get_balance(u, &token_id)).sum();

        prop_assert_eq!(contract_token_balance, tracked_sum);
        prop_assert_eq!(tracked_sum, tracked_total);
    }

    // Conservation invariant across partial settlements and holds: the contract's token
    // balance always equals the sum of tracked balances plus outstanding holds, no tracked
    // amount is ever negative, and the total supply is conserved.
    #![proptest_config(ProptestConfig::with_cases(256))]
    #[test]
    fn balances_plus_holds_equal_supply_after_partial_settlements(
        ops in prop::collection::vec(op_strategy(), 1..30)
    ) {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let contract_id = env.register_contract(None, TreasuryContract);
        let client = TreasuryContractClient::new(&env, &contract_id);
        client.initialize(&admin, &1, &soroban_sdk::Vec::new(&env));

        let token_id = env.register_stellar_asset_contract(admin.clone());
        let asset_client = token::StellarAssetClient::new(&env, &token_id);
        let token_client = token::Client::new(&env, &token_id);

        let users: Vec<Address> = (0..4).map(|_| Address::generate(&env)).collect();
        for u in &users {
            asset_client.mint(u, &10_000_000_000);
        }

        let initial_supply = token_client.total_supply();

        for op in ops {
            match op {
                Op::Deposit(who, amt) => {
                    let user = &users[(who % 4) as usize];
                    client.deposit(user, &token_id, &amt, &None);
                }
                Op::BatchDeposit(who, a, b) => {
                    let user = &users[(who % 4) as usize];
                    let mut deposits = soroban_sdk::Vec::new(&env);
                    deposits.push_back((token_id.clone(), a));
                    deposits.push_back((token_id.clone(), b));
                    client.batch_deposit(user, &deposits, &None);
                }
                Op::Withdraw(who, amt) => {
                    let user = &users[(who % 4) as usize];
                    let current: i128 = client.get_balance(user, &token_id);
                    if amt <= current {
                        client.withdraw(user, &token_id, &amt);
                    }
                }
                Op::PartialSettle(who, amt) => {
                    let user = &users[(who % 4) as usize];
                    let current: i128 = client.get_balance(user, &token_id);
                    if amt <= current {
                        client.partial_settle(user, &token_id, &amt);
                    }
                }
                Op::Hold(who, amt) => {
                    let user = &users[(who % 4) as usize];
                    let current: i128 = client.get_balance(user, &token_id);
                    if amt <= current {
                        client.hold(user, &token_id, &amt);
                    }
                }
                Op::ReleaseHold(who, amt) => {
                    let user = &users[(who % 4) as usize];
                    let held: i128 = client.get_hold(user, &token_id);
                    if amt <= held {
                        client.release_hold(user, &token_id, &amt);
                    }
                }
            }

            // Invariants must hold after every single operation.
            let tracked = tracked_balance_plus_holds(&client, &users, &token_id);
            prop_assert_eq!(token_client.balance(&contract_id), tracked);
            prop_assert_eq!(token_client.total_supply(), initial_supply);
            for u in &users {
                prop_assert!(client.get_balance(u, &token_id) >= 0);
                prop_assert!(client.get_hold(u, &token_id) >= 0);
            }
        }
    }

    // Underflow attempts must be rejected and leave the tracked state unchanged.
    #![proptest_config(ProptestConfig::with_cases(256))]
    #[test]
    fn underflow_attempts_leave_state_unchanged(
        deposit in 1i128..1_000_000,
        excess in 1i128..1_000_000,
    ) {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let contract_id = env.register_contract(None, TreasuryContract);
        let client = TreasuryContractClient::new(&env, &contract_id);
        client.initialize(&admin, &1, &soroban_sdk::Vec::new(&env));

        let token_id = env.register_stellar_asset_contract(admin.clone());
        let asset_client = token::StellarAssetClient::new(&env, &token_id);
        let token_client = token::Client::new(&env, &token_id);

        let user = Address::generate(&env);
        asset_client.mint(&user, &10_000_000_000);

        client.deposit(&user, &token_id, &deposit, &None);
        let balance_before = client.get_balance(&user, &token_id);
        let hold_before = client.get_hold(&user, &token_id);
        let contract_before = token_client.balance(&contract_id);

        let too_much = balance_before + excess;

        // Each of these must fail without mutating state.
        let withdraw_res = client.try_withdraw(&user, &token_id, &too_much);
        prop_assert!(withdraw_res.is_err());
        prop_assert_eq!(client.get_balance(&user, &token_id), balance_before);

        let settle_res = client.try_partial_settle(&user, &token_id, &too_much);
        prop_assert!(settle_res.is_err());
        prop_assert_eq!(client.get_balance(&user, &token_id), balance_before);

        let hold_res = client.try_hold(&user, &token_id, &too_much);
        prop_assert!(hold_res.is_err());
        prop_assert_eq!(client.get_hold(&user, &token_id), hold_before);

        // Releasing more than is held must also fail without mutating state.
        let release_res = client.try_release_hold(&user, &token_id, &(hold_before + excess));
        prop_assert!(release_res.is_err());
        prop_assert_eq!(client.get_hold(&user, &token_id), hold_before);

        prop_assert_eq!(client.get_balance(&user, &token_id), balance_before);
        prop_assert_eq!(token_client.balance(&contract_id), contract_before);
    }
}
