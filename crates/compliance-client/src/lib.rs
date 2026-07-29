#![no_std]

use compliance::ComplianceContractClient;
use multisig::TreasuryError;
use soroban_sdk::{Address, Env};

/// Thin ergonomic wrapper over the generated compliance contract client.
///
/// # Example
/// ```ignore
/// use compliance_client::ComplianceClient;
///
/// let client = ComplianceClient::new(&env, &compliance_contract_id);
/// client.require_allowed(&address, MyError::UnauthorizedSigner)?;
/// ```
pub struct ComplianceClient<'a> {
    inner: ComplianceContractClient<'a>,
}

impl<'a> ComplianceClient<'a> {
    pub fn new(env: &'a Env, contract_id: &Address) -> Self {
        Self {
            inner: ComplianceContractClient::new(env, contract_id),
        }
    }

    /// Convert a failed compliance check into the caller's domain error.
    pub fn require_allowed<E>(&self, address: &Address, error: E) -> Result<(), E> {
        if self.inner.is_allowed(address) {
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

impl<'a> core::ops::Deref for ComplianceClient<'a> {
    type Target = ComplianceContractClient<'a>;

    fn deref(&self) -> &Self::Target {
        &self.inner
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
        use ::compliance::ComplianceContract;
        use multisig::TreasuryError as RealTreasuryError;

        let env = Env::default();
        env.mock_all_auths();
        let admin = Address::generate(&env);
        let merchant = Address::generate(&env);

        let contract_id = env.register_contract(None, ComplianceContract);
        let client = ComplianceClient::new(&env, &contract_id);
        client.initialize(&admin);

        assert_eq!(
            client.require_allowed_for_treasury(&merchant),
            Err(RealTreasuryError::ComplianceCheckFailed)
        );

        client.allow_address(&admin, &merchant);
        assert_eq!(client.require_allowed_for_treasury(&merchant), Ok(()));
    }
}
