use crate::events;
use crate::events::{
    GraceWindowUpdatedEvent, InvoiceExpiryExtendedEvent, MaxOpenInvoicesUpdatedEvent, MerchantMaxOpenInvoicesUpdatedEvent,
};
use crate::refund::{configured_network_fee, MAX_REFUND_NETWORK_FEE};
use crate::validation::require_admin;
use crate::{
    DataKey, Invoice, InvoiceContract, InvoiceContractArgs, InvoiceContractClient, InvoiceError,
    InvoiceStatus,
};
use soroban_sdk::{contractimpl, Address, Env};

#[contractimpl]
impl InvoiceContract {
    pub fn initialize(env: Env, admin: Address) -> Result<(), InvoiceError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(InvoiceError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::InvoiceCount, &0u64);
        env.storage().instance().set(&DataKey::Paused, &false);
        Ok(())
    }

    // --- #55: configurable grace window ---

    /// Set the grace window (seconds) added to expires_at when checking payment validity.
    /// Allows a short buffer after quote expiry for in-flight payments.
    /// Emits `grace_window_updated` with old and new values.
    pub fn set_grace_window(env: Env, admin: Address, seconds: u64) -> Result<(), InvoiceError> {
        require_admin(&env, &admin)?;
        let old_seconds: u64 = env
            .storage()
            .instance()
            .get(&DataKey::GraceWindow)
            .unwrap_or(0u64);
        env.storage()
            .instance()
            .set(&DataKey::GraceWindow, &seconds);
        events::grace_window_updated(
            &env,
            &GraceWindowUpdatedEvent {
                old_seconds,
                new_seconds: seconds,
            },
        );
        Ok(())
    }

    /// Return the current grace window in seconds (0 if not set).
    pub fn get_grace_window(env: Env) -> u64 {
        env.storage()
            .instance()
            .get(&DataKey::GraceWindow)
            .unwrap_or(0u64)
    }

    // --- max open invoices limit ---

    /// Set the maximum number of open (pending) invoices a single merchant may
    /// hold at one time. Pass 0 to disable the limit (default).
    /// Emits `max_open_invoices_updated` with old and new values.
    pub fn set_max_open_invoices(env: Env, admin: Address, limit: u32) -> Result<(), InvoiceError> {
        require_admin(&env, &admin)?;
        let old_limit: u32 = env
            .storage()
            .instance()
            .get(&DataKey::MaxOpenInvoices)
            .unwrap_or(0u32);
        env.storage()
            .instance()
            .set(&DataKey::MaxOpenInvoices, &limit);
        events::max_open_invoices_updated(
            &env,
            &MaxOpenInvoicesUpdatedEvent {
                old_limit,
                new_limit: limit,
            },
        );
        Ok(())
    }

    /// Return the current max-open-invoices limit (0 = no limit).
    pub fn get_max_open_invoices(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::MaxOpenInvoices)
            .unwrap_or(0u32)
    }

    // --- #715: per-merchant open-invoice limit override ---

    /// Set a per-merchant override of the open-invoice cap. Pass `None` to clear
    /// the override, falling back to the global cap. Admin-only.
    /// Emits `merchant_max_open_invoices_updated`.
    pub fn set_merchant_max_open_invoices(
        env: Env,
        admin: Address,
        merchant: Address,
        limit: Option<u32>,
    ) -> Result<(), InvoiceError> {
        require_admin(&env, &admin)?;
        let key = DataKey::MerchantMaxOpenInvoices(merchant.clone());
        match limit {
            Some(limit) => env.storage().persistent().set(&key, &limit),
            None => env.storage().persistent().remove(&key),
        }
        events::merchant_max_open_invoices_updated(
            &env,
            &MerchantMaxOpenInvoicesUpdatedEvent {
                merchant,
                new_limit: limit,
            },
        );
        Ok(())
    }

    /// Return the effective open-invoice cap for `merchant`: the per-merchant
    /// override if one is set, otherwise the global cap (0 = no limit).
    pub fn get_max_open_invoices_for(env: Env, merchant: Address) -> u32 {
        env.storage()
            .persistent()
            .get(&DataKey::MerchantMaxOpenInvoices(merchant))
            .unwrap_or_else(|| {
                env.storage()
                    .instance()
                    .get(&DataKey::MaxOpenInvoices)
                    .unwrap_or(0u32)
            })
    }

    // --- late-fee configuration ---

    /// Set the late-fee rate in basis points (1 bps = 0.01%).
    /// Applied to overdue invoice amounts by the settlement layer.
    /// Admin-only. Pass 0 to disable the late fee (default).
    pub fn set_late_fee_bps(env: Env, admin: Address, bps: u32) -> Result<(), InvoiceError> {
        require_admin(&env, &admin)?;
        if bps > crate::refund::BPS_DENOMINATOR {
            return Err(InvoiceError::InvalidLateFee);
        }
        let old_bps: u32 = env.storage().instance().get(&DataKey::LateFee).unwrap_or(0);
        env.storage().instance().set(&DataKey::LateFee, &bps);
        // #704: make late-fee changes visible to indexers.
        events::late_fee_updated(&env, old_bps, bps);
        Ok(())
    }

    /// Return the current late-fee rate in basis points (0 if not set).
    pub fn get_late_fee_bps(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::LateFee)
            .unwrap_or(0u32)
    }

    // --- #702: configurable refund window ---

    /// Set the refund window: how many seconds after `paid_at` a payer may
    /// call `request_refund`. Pass `0` to remove the limit (current behaviour:
    /// refunds accepted at any time after payment, giving merchants indefinite
    /// exposure).
    pub fn set_refund_window(env: Env, admin: Address, seconds: u64) -> Result<(), InvoiceError> {
        require_admin(&env, &admin)?;
        env.storage()
            .instance()
            .set(&DataKey::RefundWindow, &seconds);
        Ok(())
    }

    /// Return the current refund window in seconds (0 means no limit).
    pub fn get_refund_window(env: Env) -> u64 {
        env.storage()
            .instance()
            .get(&DataKey::RefundWindow)
            .unwrap_or(0u64)
    }

    // --- #15: two-step admin transfer ---

    /// Initiate admin transfer. Current admin nominates `new_admin`.
    pub fn transfer_admin(
        env: Env,
        admin: Address,
        new_admin: Address,
    ) -> Result<(), InvoiceError> {
        require_admin(&env, &admin)?;
        env.storage()
            .instance()
            .set(&DataKey::PendingAdmin, &new_admin);
        Ok(())
    }

    /// Complete admin transfer. Must be called by the pending admin.
    pub fn accept_admin(env: Env, new_admin: Address) -> Result<(), InvoiceError> {
        new_admin.require_auth();
        let pending: Address = env
            .storage()
            .instance()
            .get(&DataKey::PendingAdmin)
            .ok_or(InvoiceError::NoPendingAdmin)?;
        if pending != new_admin {
            return Err(InvoiceError::Unauthorized);
        }
        env.storage().instance().set(&DataKey::Admin, &new_admin);
        env.storage().instance().remove(&DataKey::PendingAdmin);
        Ok(())
    }

    /// Returns the current admin address.
    pub fn get_admin(env: Env) -> Address {
        env.storage().instance().get(&DataKey::Admin).unwrap()
    }

    pub fn pause(env: Env, admin: Address) -> Result<(), InvoiceError> {
        require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::Paused, &true);
        events::contract_paused(&env, &admin);
        Ok(())
    }

    pub fn unpause(env: Env, admin: Address) -> Result<(), InvoiceError> {
        require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::Paused, &false);
        events::contract_unpaused(&env, &admin);
        Ok(())
    }

    /// Returns `true` while the contract is paused. Read-only, no auth required.
    pub fn is_paused(env: Env) -> bool {
        env.storage()
            .instance()
            .get(&DataKey::Paused)
            .unwrap_or(false)
    }

    /// Extend the expiry of a pending invoice.
    pub fn extend_expiry(
        env: Env,
        admin: Address,
        id: u64,
        additional_seconds: u64,
    ) -> Result<(), InvoiceError> {
        require_admin(&env, &admin)?;
        let mut invoice: Invoice = env
            .storage()
            .persistent()
            .get(&DataKey::Invoice(id))
            .ok_or(InvoiceError::NotFound)?;
        if invoice.status != InvoiceStatus::Pending {
            return Err(InvoiceError::NotPending);
        }

        let old_expires_at = invoice.expires_at;
        invoice.expires_at = old_expires_at
            .checked_add(additional_seconds)
            .filter(|new_expires_at| *new_expires_at > old_expires_at)
            .ok_or(InvoiceError::ExpiryOverflow)?;
        env.storage()
            .persistent()
            .set(&DataKey::Invoice(id), &invoice);
        events::invoice_expiry_extended(
            &env,
            &InvoiceExpiryExtendedEvent {
                id,
                old_expires_at,
                new_expires_at: invoice.expires_at,
            },
        );
        Ok(())
    }

    // --- #701: admin-configurable refund network fee ---

    /// Set the flat refund network fee (in stroops) deducted from refund payouts.
    /// Admin-only. Must be in `[0, MAX_REFUND_NETWORK_FEE]`.
    /// Emits `refund_network_fee_updated` with the old and new fee.
    pub fn set_refund_network_fee(env: Env, admin: Address, fee: i128) -> Result<(), InvoiceError> {
        require_admin(&env, &admin)?;
        if !(0..=MAX_REFUND_NETWORK_FEE).contains(&fee) {
            return Err(InvoiceError::InvalidAmount);
        }
        let old_fee = configured_network_fee(&env);
        env.storage()
            .instance()
            .set(&DataKey::RefundNetworkFee, &fee);
        events::refund_network_fee_updated(&env, old_fee, fee);
        Ok(())
    }

    /// Return the refund network fee in force, in stroops
    /// (`REFUND_NETWORK_FEE` when not explicitly configured).
    pub fn get_refund_network_fee(env: Env) -> i128 {
        configured_network_fee(&env)
    }
}
