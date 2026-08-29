#![no_std]

use multisig::TreasuryError;
use soroban_sdk::{contractclient, Address, Env};

/// Cross-contract call surface this crate actually needs from the compliance
/// contract. `#[contractclient]` on a bare trait (no `#[contract]`/
/// `#[contractimpl]` implementation) generates only an invocation client —
/// unlike depending on the `comebackhere-compliance` crate directly, it does
/// not compile in that contract's own wasm exports (`pause`, `unpause`, ...),
/// so contracts that depend on this crate don't collide with those exports at
/// link time. See the `compliance` dev-dependency note in Cargo.toml.
#[contractclient(name = "ComplianceOnlyClient")]
pub trait ComplianceInterface {
    fn is_allowed(env: Env, address: Address) -> bool;
}

/// Thin ergonomic wrapper over the compliance contract's `is_allowed` check.
///
/// # Example
/// ```ignore
/// use compliance_client::ComplianceClient;
///
/// let client = ComplianceClient::new(&env, &compliance_contract_id);
/// client.require_allowed(&address, MyError::UnauthorizedSigner)?;
/// ```
pub struct ComplianceClient<'a> {
    inner: ComplianceOnlyClient<'a>,
}

impl<'a> ComplianceClient<'a> {
    pub fn new(env: &'a Env, contract_id: &Address) -> Self {
        Self {
            inner: ComplianceOnlyClient::new(env, contract_id),
        }
    }

    pub fn is_allowed(&self, address: &Address) -> bool {
        self.inner.is_allowed(address)
    }

    /// Convert a failed compliance check into the caller's domain error.
    pub fn require_allowed<E>(&self, address: &Address, error: E) -> Result<(), E> {
        if self.is_allowed(address) {
            Ok(())
        } else {
            Err(error)
        }
    }

    /// Convert a failed compliance check into `TreasuryError::ComplianceCheckFailed`,
    /// following the same `From`-conversion shape established in `protocol-errors` (#72),
    /// so callers get a well-typed treasury error instead of an ad-hoc panic! or a
    /// generic `Unauthorized`.
    pub fn require_allowed_for_treasury(&self, address: &Address) -> Result<(), TreasuryError> {
        self.require_allowed(address, TreasuryError::ComplianceCheckFailed)
    }
}

#[cfg(any(test, feature = "testutils"))]
pub mod mock {
    use soroban_sdk::{Address, Env, Map};

    /// In-memory compliance double for unit tests that do not need contract invocation.
    pub struct MockComplianceClient {
        responses: Map<Address, bool>,
        default_allowed: bool,
    }

    impl MockComplianceClient {
        pub fn new(env: &Env, default_allowed: bool) -> Self {
            Self {
                responses: Map::new(env),
                default_allowed,
            }
        }

        pub fn set_allowed(&mut self, address: &Address, allowed: bool) {
            self.responses.set(address.clone(), allowed);
        }

        pub fn is_allowed(&self, address: &Address) -> bool {
            self.responses
                .get(address.clone())
                .unwrap_or(self.default_allowed)
        }

        pub fn require_allowed<E>(&self, address: &Address, error: E) -> Result<(), E> {
            if self.is_allowed(address) {
                Ok(())
            } else {
                Err(error)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::mock::MockComplianceClient;
    use soroban_sdk::{testutils::Address as _, Address, Env};

    #[derive(Debug, Eq, PartialEq)]
    enum TreasuryError {
        UnauthorizedSigner,
    }

    #[test]
    fn mock_produces_treasury_pass_and_fail_responses() {
        let env = Env::default();
        let allowed = Address::generate(&env);
        let denied = Address::generate(&env);
        let mut compliance = MockComplianceClient::new(&env, false);
        compliance.set_allowed(&allowed, true);

        assert_eq!(
            compliance.require_allowed(&allowed, TreasuryError::UnauthorizedSigner),
            Ok(())
        );
        assert_eq!(
            compliance.require_allowed(&denied, TreasuryError::UnauthorizedSigner),
            Err(TreasuryError::UnauthorizedSigner)
        );
    }

    #[test]
    fn require_allowed_for_treasury_maps_compliance_gate_failure() {
        use crate::ComplianceClient;
        use ::compliance::{ComplianceContract, ComplianceContractClient};
        use multisig::TreasuryError as RealTreasuryError;

        let env = Env::default();
        env.mock_all_auths();
        let admin = Address::generate(&env);
        let merchant = Address::generate(&env);

        let contract_id = env.register_contract(None, ComplianceContract);
        // Real generated client, used only to drive compliance contract state.
        let admin_client = ComplianceContractClient::new(&env, &contract_id);
        admin_client.initialize(&admin);

        let client = ComplianceClient::new(&env, &contract_id);
        assert_eq!(
            client.require_allowed_for_treasury(&merchant),
            Err(RealTreasuryError::ComplianceCheckFailed)
        );

        admin_client.allow_address(&admin, &merchant);
        assert_eq!(client.require_allowed_for_treasury(&merchant), Ok(()));
    }
}
