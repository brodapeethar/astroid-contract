#![no_std]
#![allow(clippy::too_many_arguments)]
//! # Astroid Treasury Contract
//!
//! Custodies organizational funds and enforces governance on every outbound
//! movement (PRD Doc 7 §Treasury). Every `withdraw` / `transfer` resolves the
//! organization's Policy and Budget contracts and calls them BEFORE debiting
//! the ledger, so a spend must satisfy:
//!
//! ```text
//! admin auth → policy.check_transfer → budget.consume → assets move
//! ```
//!
//! Cross-contract calls go through the typed clients generated from
//! [`astroid_interfaces`], keeping the graph acyclic: `Treasury → {Policy, Budget}`.
//!
//! [`TreasuryContract::batch_transfer`] applies the same gate chain to a whole
//! vector of payouts in a single, atomic invocation: the cumulative amount is
//! accumulated with checked math and validated against the treasury balance
//! before any value moves, so autonomous agents can pay many contributors for
//! the fee of one transaction. If any leg fails, the host reverts the entire
//! invocation and no recipient is paid.
//!
//! Functions: `initialize`, `set_policy`, `set_budget`, `freeze`, `unfreeze`,
//! `deposit`, `withdraw`, `batch_transfer`, `allocate_budget`, `get`, `holding`.

use astroid_interfaces::PolicyClient;
use astroid_shared::constants::{
    INSTANCE_BUMP_AMOUNT, INSTANCE_LIFETIME_THRESHOLD, MAX_BATCH_PAYMENTS, PERSISTENT_BUMP_AMOUNT,
    PERSISTENT_LIFETIME_THRESHOLD,
};
use astroid_shared::errors::Error;
use astroid_shared::events;
use astroid_shared::math::{checked_add, checked_sub};
use astroid_shared::types::{Payment, ResourceState};
use astroid_shared::validation::{require_non_empty, require_positive_amount};
use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, token, Address, Env, String, Vec,
};

/// Stored treasury record.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Treasury {
    pub org: String,
    pub admin: Address,
    /// Organization's Policy contract — consulted on every spend.
    pub policy: Option<Address>,
    /// Organization's Budget contract root.
    pub budget: Option<Address>,
    /// Lifecycle state shared with wallets.
    pub state: ResourceState,
}

/// Per-asset accounting within the treasury.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Holding {
    pub asset: Address,
    pub total_in: i128,
    pub total_out: i128,
    /// Budget envelope backing this asset, if any.
    pub budget_id: Option<String>,
}

#[contracttype]
#[derive(Clone)]
enum DataKey {
    Treasury,
    Holding(Address),
}

#[contract]
pub struct TreasuryContract;

#[contractimpl]
impl TreasuryContract {
    /// Create a treasury for `org`, gated on the admin's signature.
    pub fn initialize(env: Env, org: String, admin: Address) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Treasury) {
            return Err(Error::AlreadyInitialized);
        }
        require_non_empty(&org)?;
        env.storage().instance().set(
            &DataKey::Treasury,
            &Treasury {
                org: org.clone(),
                admin: admin.clone(),
                policy: None,
                budget: None,
                state: ResourceState::Active,
            },
        );
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
        events::treasury_created(&env, &org, &admin);
        Ok(())
    }

    /// Wire the policy-enforcement contract consulted before every spend.
    pub fn set_policy(env: Env, caller: Address, policy: Address) -> Result<(), Error> {
        let mut t = Self::require_admin(&env, &caller)?;
        t.policy = Some(policy);
        Self::store(&env, &t);
        events::publish(
            &env,
            events::ContractEvent::TreasuryConfigUpdated {
                org: t.org.clone(),
                action: symbol_short!("policy"),
            },
        );
        env.events()
            .publish((symbol_short!("treasury"), symbol_short!("policy")), ());
        Ok(())
    }

    /// Wire the budget-tracking contract backing this treasury.
    pub fn set_budget(env: Env, caller: Address, budget: Address) -> Result<(), Error> {
        let mut t = Self::require_admin(&env, &caller)?;
        t.budget = Some(budget);
        Self::store(&env, &t);
        events::publish(
            &env,
            events::ContractEvent::TreasuryConfigUpdated {
                org: t.org.clone(),
                action: symbol_short!("budget"),
            },
        );
        env.events()
            .publish((symbol_short!("treasury"), symbol_short!("budget")), ());
        Ok(())
    }

    /// Freeze the treasury; all outflows are rejected while frozen.
    pub fn freeze(env: Env, caller: Address) -> Result<(), Error> {
        let mut t = Self::require_admin(&env, &caller)?;
        t.state = ResourceState::Frozen;
        Self::store(&env, &t);
        events::publish(
            &env,
            events::ContractEvent::TreasuryConfigUpdated {
                org: t.org.clone(),
                action: symbol_short!("frozen"),
            },
        );
        env.events()
            .publish((symbol_short!("treasury"), symbol_short!("frozen")), ());
        Ok(())
    }

    /// Unfreeze back to active.
    pub fn unfreeze(env: Env, caller: Address) -> Result<(), Error> {
        let mut t = Self::require_admin(&env, &caller)?;
        if t.state != ResourceState::Frozen {
            return Err(Error::InvalidState);
        }
        t.state = ResourceState::Active;
        Self::store(&env, &t);
        events::publish(
            &env,
            events::ContractEvent::TreasuryConfigUpdated {
                org: t.org.clone(),
                action: symbol_short!("unfrozen"),
            },
        );
        env.events()
            .publish((symbol_short!("treasury"), symbol_short!("unfrozen")), ());
        Ok(())
    }

    /// Deposit assets into the treasury (any funder may authorize). Moves real
    /// SAC tokens from `from` into the treasury's custody, then credits the
    /// internal per-asset accounting.
    pub fn deposit(env: Env, from: Address, asset: Address, amount: i128) -> Result<(), Error> {
        require_positive_amount(amount)?;
        from.require_auth();
        let t = Self::load(&env);
        Self::require_active(&t)?;
        // Pull tokens into the contract's own custody.
        token::TokenClient::new(&env, &asset).transfer(
            &from,
            &env.current_contract_address(),
            &amount,
        );
        let mut h = Self::load_holding(&env, &asset);
        h.total_in = checked_add(h.total_in, amount)?;
        Self::store_holding(&env, &asset, &h);
        env.events().publish(
            (symbol_short!("treasury"), symbol_short!("deposited")),
            (asset, amount),
        );
        Ok(())
    }

    /// Attach a budget envelope to an asset (admin).
    pub fn allocate_budget(
        env: Env,
        admin: Address,
        asset: Address,
        budget_id: String,
    ) -> Result<(), Error> {
        let _t = Self::require_admin(&env, &admin)?;
        require_non_empty(&budget_id)?;
        let mut h = Self::load_holding(&env, &asset);
        h.budget_id = Some(budget_id);
        Self::store_holding(&env, &asset, &h);
        Ok(())
    }

    /// Withdraw assets to a recipient. Only the admin may call, and the spend
    /// must clear policy and budget gates before the ledger is debited.
    pub fn withdraw(
        env: Env,
        caller: Address,
        asset: Address,
        to: Address,
        amount: i128,
    ) -> Result<(), Error> {
        require_positive_amount(amount)?;
        let t = Self::load(&env);
        Self::require_active(&t)?;
        if t.admin != caller {
            return Err(Error::Unauthorized);
        }
        caller.require_auth();

        // 1. Policy verification — the policy contract evaluates the spend.
        if let Some(policy_addr) = &t.policy {
            PolicyClient::new(&env, policy_addr).check_transfer(
                &String::from_str(&env, "active"),
                &asset,
                &to,
                &amount,
            );
        }

        // 2. Budget consumption — aborts if the envelope lacks headroom.
        let mut holding = Self::load_holding(&env, &asset);
        if let (Some(budget_addr), Some(budget_id)) = (&t.budget, &holding.budget_id) {
            astroid_interfaces::BudgetClient::new(&env, budget_addr)
                .consume(&caller, budget_id, &amount);
        }

        // 3. Debit the internal ledger, then move real tokens out of custody.
        if holding.total_in < amount {
            return Err(Error::InsufficientFunds);
        }
        holding.total_in = checked_sub(holding.total_in, amount)?;
        holding.total_out = checked_add(holding.total_out, amount)?;
        Self::store_holding(&env, &asset, &holding);
        token::TokenClient::new(&env, &asset).transfer(
            &env.current_contract_address(),
            &to,
            &amount,
        );
        events::transfer_executed(&env, &t.admin, &to, &asset, amount);
        events::publish(
            &env,
            events::ContractEvent::TransferExecuted {
                from: t.admin.clone(),
                to: to.clone(),
                asset: asset.clone(),
                amount,
            },
        );
        Ok(())
    }

    /// Disburse `payments` of a single `asset` to many recipients in one atomic
    /// transaction. Only the admin may call, and the batch clears exactly the
    /// same gates as [`TreasuryContract::withdraw`] — policy per leg, budget for
    /// the aggregate — before the ledger is debited.
    ///
    /// The payout total is accumulated with the shared checked-math helpers and
    /// verified against the treasury's recorded balance up front, so an
    /// over-drawing batch is rejected before any token moves. Beyond that,
    /// atomicity is guaranteed by the host: returning an error (or a failing
    /// sub-call, such as a policy denial or a token transfer) rolls back every
    /// storage write and every transfer made earlier in the invocation, so a
    /// batch either pays every recipient or none of them.
    pub fn batch_transfer(
        env: Env,
        caller: Address,
        asset: Address,
        payments: Vec<Payment>,
    ) -> Result<(), Error> {
        if payments.is_empty() || payments.len() > MAX_BATCH_PAYMENTS {
            return Err(Error::InvalidInput);
        }
        let t = Self::load(&env);
        Self::require_active(&t)?;
        if t.admin != caller {
            return Err(Error::Unauthorized);
        }
        caller.require_auth();

        // 1. Validate every leg and accumulate the payout with checked math, so
        //    a malformed or overflowing batch is rejected before anything moves.
        let mut total: i128 = 0;
        for payment in payments.iter() {
            require_positive_amount(payment.amount)?;
            total = checked_add(total, payment.amount)?;
        }

        // 2. Cumulative balance check against the recorded holding.
        let mut holding = Self::load_holding(&env, &asset);
        if holding.total_in < total {
            return Err(Error::InsufficientFunds);
        }

        // 3. Policy verification — each leg is evaluated on its own, because
        //    per-recipient and per-amount gates are what the policy encodes.
        if let Some(policy_addr) = &t.policy {
            let policy = PolicyClient::new(&env, policy_addr);
            let policy_id = String::from_str(&env, "active");
            for payment in payments.iter() {
                policy.check_transfer(&policy_id, &asset, &payment.recipient, &payment.amount);
            }
        }

        // 4. Budget consumption — one debit for the aggregate rather than one
        //    cross-contract call per recipient.
        if let (Some(budget_addr), Some(budget_id)) = (&t.budget, &holding.budget_id) {
            astroid_interfaces::BudgetClient::new(&env, budget_addr)
                .consume(&caller, budget_id, &total);
        }

        // 5. Debit the internal ledger once, then move real tokens per recipient.
        holding.total_in = checked_sub(holding.total_in, total)?;
        holding.total_out = checked_add(holding.total_out, total)?;
        Self::store_holding(&env, &asset, &holding);

        let token_client = token::TokenClient::new(&env, &asset);
        let custody = env.current_contract_address();
        for payment in payments.iter() {
            token_client.transfer(&custody, &payment.recipient, &payment.amount);
        }

        // A single summary event keeps the log concise; the per-recipient moves
        // are already observable as the asset contract's own transfer events.
        events::publish(
            &env,
            events::ContractEvent::BatchTransferExecuted {
                from: t.admin.clone(),
                asset: asset.clone(),
                count: payments.len(),
                total,
            },
        );
        env.events().publish(
            (symbol_short!("treasury"), symbol_short!("batchpay")),
            (asset, payments.len(), total),
        );
        Ok(())
    }

    // --- views ---

    pub fn get(env: Env) -> Treasury {
        Self::load(&env)
    }

    pub fn holding(env: Env, asset: Address) -> Holding {
        Self::load_holding(&env, &asset)
    }

    // --- internals ---

    fn load(env: &Env) -> Treasury {
        env.storage()
            .instance()
            .get(&DataKey::Treasury)
            .expect("treasury not initialized")
    }

    fn store(env: &Env, t: &Treasury) {
        env.storage().instance().set(&DataKey::Treasury, t);
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    fn require_admin(env: &Env, caller: &Address) -> Result<Treasury, Error> {
        let t = Self::load(env);
        if t.admin != *caller {
            return Err(Error::Unauthorized);
        }
        caller.require_auth();
        Ok(t)
    }

    fn require_active(t: &Treasury) -> Result<(), Error> {
        match t.state {
            ResourceState::Active => Ok(()),
            _ => Err(Error::InvalidState),
        }
    }

    fn load_holding(env: &Env, asset: &Address) -> Holding {
        env.storage()
            .persistent()
            .get(&DataKey::Holding(asset.clone()))
            .unwrap_or(Holding {
                asset: asset.clone(),
                total_in: 0,
                total_out: 0,
                budget_id: None,
            })
    }

    fn store_holding(env: &Env, asset: &Address, h: &Holding) {
        env.storage()
            .persistent()
            .set(&DataKey::Holding(asset.clone()), h);
        env.storage().persistent().extend_ttl(
            &DataKey::Holding(asset.clone()),
            PERSISTENT_LIFETIME_THRESHOLD,
            PERSISTENT_BUMP_AMOUNT,
        );
    }
}

#[cfg(test)]
mod test;
