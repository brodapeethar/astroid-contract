#![no_std]
#![allow(clippy::too_many_arguments)]
//! # Astroid Policy Contract
//!
//! Verifies that a proposed transfer complies with the ACTIVE policy
//! configuration (PRD Doc 7 §Policy). The Astroid backend owns the human-facing
//! policy graph; this contract stores only a cryptographic hash of the active
//! configuration and a small set of scalar gates so on-chain verification is
//! cheap, fast and tamper-evident (PRD "Policy Hash Verification" enhancement).
//!
//! ```text
//! off-chain policy.json → hash → store on-chain
//! transaction → recompute hash of ACTIVE config → compare → allow / deny
//! ```
//!
//! This contract answers: "may `amount` of `asset` flow to `recipient`
//! right now?" with a deterministic [`Error`] when it may not.
//!
//! Functions: `initialize`, `register_policy`, `rotate_policy`, `check_transfer`.

use astroid_interfaces::PolicyInterface;
use astroid_shared::errors::Error;
use astroid_shared::events::ContractEvent;
use astroid_shared::validation::require_non_empty;
use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, Address, BytesN, Env, String,
};

/// On-chain representation of a registered policy.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Policy {
    /// Admin that controls this policy (typically the treasury/admin wallet).
    pub owner: Address,
    /// SHA-256 hash of the human-readable policy JSON managed off-chain.
    pub config_hash: BytesN<32>,
    /// Scalar gates baked in for cheap on-chain checks (so we don't need JSON).
    pub max_amount: i128,
    /// Allow-listed recipient (zero-length means "any" is allowed).
    pub allowed_recipient: Option<Address>,
    /// Asset contract address the spend must be in (None = any asset).
    pub allowed_asset: Option<Address>,
    /// Unix timestamp the policy is active until (0 = no expiry).
    pub expires_at: u64,
    /// Whether the policy is currently enabled.
    pub enabled: bool,
}

#[contracttype]
#[derive(Clone)]
enum DataKey {
    Policy(String),
    Count,
    Blacklist(Address),
    MerchantBlacklist(Address),
    CategoryBlacklist(String),
    /// Per-policy asset whitelist: (policy_id, asset) -> true.
    AssetWhitelist(String, Address),
    /// Whether an org uses a permissive (all-assets-allowed) or restrictive
    /// (whitelist-enforced) asset mode. Stored per policy_id.
    AssetWhitelistEnabled(String),
}

#[contract]
pub struct PolicyContract;

#[contractimpl]
impl PolicyContract {
    pub fn initialize(env: Env) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Count) {
            return Err(Error::AlreadyInitialized);
        }
        env.storage().instance().set(&DataKey::Count, &0u32);
        Ok(())
    }

    /// Register a policy. `owner` gates subsequent rotations. Cheap scalar gates
    /// are stored on-chain; the full configuration is hashed for tamper-evidence.
    #[allow(clippy::too_many_arguments)]
    pub fn register_policy(
        env: Env,
        owner: Address,
        policy_id: String,
        config_hash: BytesN<32>,
        max_amount: i128,
        allowed_recipient: Option<Address>,
        allowed_asset: Option<Address>,
        expires_at: u64,
    ) -> Result<(), Error> {
        owner.require_auth();
        require_non_empty(&policy_id)?;
        if env
            .storage()
            .persistent()
            .has(&DataKey::Policy(policy_id.clone()))
        {
            return Err(Error::AlreadyExists);
        }
        let policy = Policy {
            owner,
            config_hash,
            max_amount,
            allowed_recipient,
            allowed_asset,
            expires_at,
            enabled: true,
        };
        env.storage()
            .persistent()
            .set(&DataKey::Policy(policy_id.clone()), &policy);
        env.events().publish(
            (symbol_short!("policy"), symbol_short!("registd")),
            policy_id,
        );
        Ok(())
    }

    /// Rotate an existing policy hash — e.g. after the backend recomputes it.
    pub fn rotate_policy(
        env: Env,
        caller: Address,
        policy_id: String,
        new_hash: BytesN<32>,
        new_max: i128,
    ) -> Result<(), Error> {
        caller.require_auth();
        let mut policy = Self::load(&env, &policy_id)?;
        if policy.owner != caller {
            return Err(Error::Unauthorized);
        }
        policy.config_hash = new_hash;
        policy.max_amount = new_max;
        env.storage()
            .persistent()
            .set(&DataKey::Policy(policy_id.clone()), &policy);
        env.events().publish(
            (symbol_short!("policy"), symbol_short!("rotated")),
            policy_id,
        );
        Ok(())
    }

    /// Disable / enable a policy (owner only).
    pub fn set_enabled(
        env: Env,
        caller: Address,
        policy_id: String,
        enabled: bool,
    ) -> Result<(), Error> {
        caller.require_auth();
        let mut policy = Self::load(&env, &policy_id)?;
        if policy.owner != caller {
            return Err(Error::Unauthorized);
        }
        policy.enabled = enabled;
        env.storage()
            .persistent()
            .set(&DataKey::Policy(policy_id.clone()), &policy);
        Ok(())
    }

    /// Add an asset to the policy's whitelist (owner only). When the asset
    /// whitelist is enabled for a policy, only whitelisted assets are permitted
    /// in `check_transfer`.
    pub fn add_asset_to_whitelist(
        env: Env,
        caller: Address,
        policy_id: String,
        asset: Address,
    ) -> Result<(), Error> {
        caller.require_auth();
        let policy = Self::load(&env, &policy_id)?;
        if policy.owner != caller {
            return Err(Error::Unauthorized);
        }
        let key = DataKey::AssetWhitelist(policy_id.clone(), asset.clone());
        if env.storage().persistent().has(&key) {
            return Err(Error::AlreadyExists);
        }
        env.storage().persistent().set(&key, &true);
        env.events().publish(
            (symbol_short!("policy"), symbol_short!("asset_add")),
            (policy_id, asset),
        );
        Ok(())
    }

    /// Remove an asset from the policy's whitelist (owner only).
    pub fn remove_asset_from_whitelist(
        env: Env,
        caller: Address,
        policy_id: String,
        asset: Address,
    ) -> Result<(), Error> {
        caller.require_auth();
        let policy = Self::load(&env, &policy_id)?;
        if policy.owner != caller {
            return Err(Error::Unauthorized);
        }
        let key = DataKey::AssetWhitelist(policy_id.clone(), asset.clone());
        if !env.storage().persistent().has(&key) {
            return Err(Error::NotFound);
        }
        env.storage().persistent().remove(&key);
        env.events().publish(
            (symbol_short!("policy"), symbol_short!("asset_rem")),
            (policy_id, asset),
        );
        Ok(())
    }

    /// Enable or disable the asset whitelist for a policy (owner only).
    /// When enabled, only assets explicitly added via `add_asset_to_whitelist`
    /// are permitted.
    pub fn set_asset_whitelist_enabled(
        env: Env,
        caller: Address,
        policy_id: String,
        enabled: bool,
    ) -> Result<(), Error> {
        caller.require_auth();
        let policy = Self::load(&env, &policy_id)?;
        if policy.owner != caller {
            return Err(Error::Unauthorized);
        }
        let key = DataKey::AssetWhitelistEnabled(policy_id);
        env.storage().persistent().set(&key, &enabled);
        Ok(())
    }

    /// Check whether an asset is whitelisted for a given policy.
    /// Returns Ok(()) if allowed, or AssetNotWhitelisted if the whitelist is
    /// enabled and the asset is not present.
    pub fn validate_asset(env: Env, policy_id: String, asset: Address) -> Result<(), Error> {
        let enabled_key = DataKey::AssetWhitelistEnabled(policy_id.clone());
        let whitelist_enabled: bool = env
            .storage()
            .persistent()
            .get(&enabled_key)
            .unwrap_or(false);
        if !whitelist_enabled {
            return Ok(());
        }
        let key = DataKey::AssetWhitelist(policy_id.clone(), asset.clone());
        if !env.storage().persistent().has(&key) {
            events_policy_violation(&env, &policy_id, "asset_not_whitelisted");
            return Err(Error::AssetNotWhitelisted);
        }
        Ok(())
    }

    /// Add an address to the restricted blacklist (owner only).
    pub fn add_blacklist(
        env: Env,
        caller: Address,
        policy_id: String,
        address: Address,
    ) -> Result<(), Error> {
        caller.require_auth();
        let policy = Self::load(&env, &policy_id)?;
        if policy.owner != caller {
            return Err(Error::Unauthorized);
        }
        let key = DataKey::Blacklist(address.clone());
        if env.storage().persistent().has(&key) {
            return Err(Error::AlreadyExists);
        }
        env.storage().persistent().set(&key, &policy_id);
        env.events().publish(
            (symbol_short!("policy"), symbol_short!("blk_add")),
            (policy_id, address),
        );
        Ok(())
    }

    /// Remove an address from the restricted blacklist (owner only).
    pub fn remove_blacklist(
        env: Env,
        caller: Address,
        policy_id: String,
        address: Address,
    ) -> Result<(), Error> {
        caller.require_auth();
        let policy = Self::load(&env, &policy_id)?;
        if policy.owner != caller {
            return Err(Error::Unauthorized);
        }
        let key = DataKey::Blacklist(address.clone());
        if !env.storage().persistent().has(&key) {
            return Err(Error::NotFound);
        }
        env.storage().persistent().remove(&key);
        env.events().publish(
            (symbol_short!("policy"), symbol_short!("blk_rem")),
            (policy_id, address),
        );
        Ok(())
    }

    /// Add a merchant address to the merchant blacklist (owner only).
    pub fn add_merchant_blacklist(
        env: Env,
        caller: Address,
        policy_id: String,
        merchant_address: Address,
    ) -> Result<(), Error> {
        caller.require_auth();
        let policy = Self::load(&env, &policy_id)?;
        if policy.owner != caller {
            return Err(Error::Unauthorized);
        }
        let key = DataKey::MerchantBlacklist(merchant_address.clone());
        if env.storage().persistent().has(&key) {
            return Err(Error::AlreadyExists);
        }
        env.storage().persistent().set(&key, &policy_id);
        env.events().publish(
            (symbol_short!("policy"), symbol_short!("merch_add")),
            (policy_id, merchant_address),
        );
        Ok(())
    }

    /// Remove a merchant address from the merchant blacklist (owner only).
    pub fn remove_merchant_blacklist(
        env: Env,
        caller: Address,
        policy_id: String,
        merchant_address: Address,
    ) -> Result<(), Error> {
        caller.require_auth();
        let policy = Self::load(&env, &policy_id)?;
        if policy.owner != caller {
            return Err(Error::Unauthorized);
        }
        let key = DataKey::MerchantBlacklist(merchant_address.clone());
        if !env.storage().persistent().has(&key) {
            return Err(Error::NotFound);
        }
        env.storage().persistent().remove(&key);
        env.events().publish(
            (symbol_short!("policy"), symbol_short!("merch_rem")),
            (policy_id, merchant_address),
        );
        Ok(())
    }

    /// Add a spending category to the category blacklist (owner only).
    pub fn add_category_blacklist(
        env: Env,
        caller: Address,
        policy_id: String,
        category: String,
    ) -> Result<(), Error> {
        caller.require_auth();
        let policy = Self::load(&env, &policy_id)?;
        if policy.owner != caller {
            return Err(Error::Unauthorized);
        }
        require_non_empty(&category)?;
        let key = DataKey::CategoryBlacklist(category.clone());
        if env.storage().persistent().has(&key) {
            return Err(Error::AlreadyExists);
        }
        env.storage().persistent().set(&key, &policy_id);
        env.events().publish(
            (symbol_short!("policy"), symbol_short!("cat_add")),
            (policy_id, category),
        );
        Ok(())
    }

    /// Remove a spending category from the category blacklist (owner only).
    pub fn remove_category_blacklist(
        env: Env,
        caller: Address,
        policy_id: String,
        category: String,
    ) -> Result<(), Error> {
        caller.require_auth();
        let policy = Self::load(&env, &policy_id)?;
        if policy.owner != caller {
            return Err(Error::Unauthorized);
        }
        let key = DataKey::CategoryBlacklist(category.clone());
        if !env.storage().persistent().has(&key) {
            return Err(Error::NotFound);
        }
        env.storage().persistent().remove(&key);
        env.events().publish(
            (symbol_short!("policy"), symbol_short!("cat_rem")),
            (policy_id, category),
        );
        Ok(())
    }

    /// Add a recipient address to the blocklist (owner only). Blocked
    /// addresses are rejected immediately in `check_transfer` before any
    /// other policy gate is evaluated (Issue #32).
    pub fn add_to_blocklist(
        env: Env,
        caller: Address,
        policy_id: String,
        address: Address,
    ) -> Result<(), Error> {
        caller.require_auth();
        let policy = Self::load(&env, &policy_id)?;
        if policy.owner != caller {
            return Err(Error::Unauthorized);
        }
        let key = DataKey::Blacklist(address.clone());
        if env.storage().persistent().has(&key) {
            return Err(Error::AlreadyExists);
        }
        env.storage().persistent().set(&key, &policy_id);
        env.events().publish(
            (symbol_short!("policy"), symbol_short!("blk_add")),
            (policy_id, address),
        );
        Ok(())
    }

    /// Remove a recipient address from the blocklist (owner only).
    pub fn remove_from_blocklist(
        env: Env,
        caller: Address,
        policy_id: String,
        address: Address,
    ) -> Result<(), Error> {
        caller.require_auth();
        let policy = Self::load(&env, &policy_id)?;
        if policy.owner != caller {
            return Err(Error::Unauthorized);
        }
        let key = DataKey::Blacklist(address.clone());
        if !env.storage().persistent().has(&key) {
            return Err(Error::NotFound);
        }
        env.storage().persistent().remove(&key);
        env.events().publish(
            (symbol_short!("policy"), symbol_short!("blk_rem")),
            (policy_id, address),
        );
        Ok(())
    }

    /// Check if a spending category is restricted. Returns Ok(()) if the category
    /// is allowed, or PolicyCategoryRestricted if it's blacklisted.
    pub fn check_category(env: Env, policy_id: String, category: String) -> Result<(), Error> {
        // Empty category is always allowed
        if category.is_empty() {
            return Ok(());
        }

        if env
            .storage()
            .persistent()
            .has(&DataKey::CategoryBlacklist(category.clone()))
        {
            events_policy_violation(&env, &policy_id, "category_restricted");
            return Err(Error::PolicyCategoryRestricted);
        }
        Ok(())
    }

    // --- views ---

    pub fn get(env: Env, policy_id: String) -> Result<Policy, Error> {
        Self::load(&env, &policy_id)
    }

    // --- internels ---

    fn load(env: &Env, id: &String) -> Result<Policy, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Policy(id.clone()))
            .ok_or(Error::NotFound)
    }
}

/// Allow the interface trait to call `check_transfer` on this contract.
#[contractimpl]
impl PolicyInterface for PolicyContract {
    /// Evaluate a transfer request against the named policy. All gates must pass.
    ///
    /// Blocklist checks run **first** so that compromised or malicious
    /// addresses are rejected immediately, before any allowance, asset or
    /// amount evaluation (Issue #32).
    fn check_transfer(
        env: Env,
        policy_id: String,
        asset: Address,
        recipient: Address,
        amount: i128,
    ) -> Result<(), Error> {
        let policy = Self::load(&env, &policy_id)?;
        // Disabled policies deny every spend.
        if !policy.enabled {
            events_policy_violation(&env, &policy_id, "disabled");
            return Err(Error::PolicyDenied);
        }
        // --- Blocklist checks (Issue #32) — evaluated first ---
        if env
            .storage()
            .persistent()
            .has(&DataKey::Blacklist(recipient.clone()))
        {
            events_policy_violation(&env, &policy_id, "blacklisted");
            return Err(Error::PolicyRecipientRestricted);
        }
        if env
            .storage()
            .persistent()
            .has(&DataKey::MerchantBlacklist(recipient.clone()))
        {
            events_policy_violation(&env, &policy_id, "merchant_blocked");
            return Err(Error::PolicyMerchantBlocked);
        }
        // --- Allowance / amount gates ---
        if policy.expires_at != 0 && env.ledger().timestamp() >= policy.expires_at {
            events_policy_violation(&env, &policy_id, "expired");
            return Err(Error::PolicyDenied);
        }
        if policy.max_amount != 0 && amount > policy.max_amount {
            events_policy_violation(&env, &policy_id, "above_max");
            return Err(Error::PolicyDenied);
        }
        if let Some(allow_recip) = &policy.allowed_recipient {
            if allow_recip.clone() != recipient {
                events_policy_violation(&env, &policy_id, "bad_recipient");
                return Err(Error::PolicyDenied);
            }
        }
        if let Some(allow_asset) = &policy.allowed_asset {
            if allow_asset.clone() != asset {
                events_policy_violation(&env, &policy_id, "bad_asset");
                return Err(Error::PolicyDenied);
            }
        }
        // Check asset whitelist (Issue #37)
        Self::validate_asset(env.clone(), policy_id.clone(), asset.clone())?;
        // Check blacklist
        if env
            .storage()
            .persistent()
            .has(&DataKey::Blacklist(recipient.clone()))
        {
            events_policy_violation(&env, &policy_id, "blacklisted");
            return Err(Error::PolicyRecipientRestricted);
        }
        // Check merchant blacklist
        if env
            .storage()
            .persistent()
            .has(&DataKey::MerchantBlacklist(recipient.clone()))
        {
            events_policy_violation(&env, &policy_id, "merchant_blocked");
            return Err(Error::PolicyMerchantBlocked);
        }
        Ok(())
    }
}

/// Emit a `PolicyViolation` event with a stable reason symbol, using both the
/// legacy tuple-topic helper and the canonical [`ContractEvent`] schema.
fn events_policy_violation(env: &Env, policy_id: &String, reason: &str) {
    let r = soroban_sdk::Symbol::new(env, reason);
    astroid_shared::events::policy_violation(env, policy_id, r.clone());
    astroid_shared::events::publish(
        env,
        ContractEvent::PolicyViolation {
            policy_id: policy_id.clone(),
            reason: r,
        },
    );
}

#[cfg(test)]
mod test;
