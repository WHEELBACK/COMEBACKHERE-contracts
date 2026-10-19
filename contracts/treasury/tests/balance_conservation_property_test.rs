use proptest::prelude::*;
use soroban_sdk::{testutils::Address as _, token, Address, Env};
use treasury::{TreasuryContract, TreasuryContractClient};

#[derive(Clone, Debug)]
enum Op {
    Deposit(u8, i128),
    BatchDeposit(u8, i128, i128),
    Withdraw(u8, i128),
}

fn op_strategy() -> impl Strategy<Value = Op> {
    prop_oneof![
        (0u8..4, 1i128..1_000_000).prop_map(|(who, amt)| Op::Deposit(who, amt)),
        (0u8..4, 1i128..500_000, 1i128..500_000)
            .prop_map(|(who, a, b)| Op::BatchDeposit(who, a, b)),
        (0u8..4, 1i128..1_000_000).prop_map(|(who, amt)| Op::Withdraw(who, amt)),
    ]
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
}
