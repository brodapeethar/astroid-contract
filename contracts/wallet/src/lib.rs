#![no_std]
//! # Astroid Wallet Contract
//!
//! Programmable, stateful custody wallets for AI agents. The contract is the
//! on-chain custodian: real assets (Stellar Asset Contract tokens) are held at
//! the wallet contract's own address, while per-wallet balances are tracked in
//! internal bookkeeping so an individual wallet can never spend more than it
//! holds.
//!
//! Lifecycle states (PRD Doc 7 §Wallet): `Active`, `Frozen`, `Paused`,
//! `Archived`. Outbound value movement is only permitted from an `Active`
//! wallet; every other state fails safely with a specific error.
//!
//! Functions: `create_wallet`, `deposit`, `transfer`, `withdraw`, `freeze`,
//! `unfreeze`, `pause`, `unpause`, `archive`, `dispatch`.
//!
//! Events: `WalletCreated`, `WalletFrozen`, `TransferExecuted` (shared schema)
//! plus wallet-scoped state-change events.
//!
//! ## Dispatch & Authorization
//!
//! The `dispatch` entrypoint allows registered organizational modules (e.g.
//! multisig, treasury, policy) to execute wallet operations on behalf of the
//! organization. Every dispatch call is verified against the on-chain registry:
//! the caller must be a registered module address for this wallet's
//! organization, or the wallet owner / admin. Unauthorized dispatch attempts
//! fail with [`Error::UnauthorizedDispatch`].

use astroid_interfaces::RegistryClient;
use astroid_shared::constants::{INSTANCE_BUMP_AMOUNT, INSTANCE_LIFETIME_THRESHOLD};
use astroid_shared::errors::Error;
use astroid_shared::math::{checked_add, checked_sub};
use astroid_shared::types::{ModuleKind, ResourceState};
use astroid_shared::validation::require_positive_amount;
use astroid_shared::{constants, events};
use soroban_sdk::{contract, contractimpl, contracttype, symbol_short, token, Address, Env, String};

#[contracttype]
#[derive(Clone)]
enum DataKey {
    /// Emergency/administrative address able to freeze any wallet (instance).
    Admin,
    /// Monotonic wallet id counter (instance).
    WalletCount,
    /// Wallet record: id -> WalletData.
    Wallet(u64),
    /// Per-wallet, per-asset balance: (id, asset) -> i128.
    Balance(u64, Address),
    /// Registry contract address for caller verification (instance).
    Registry,
    /// Organization slug this wallet belongs to (instance).
    Org,
}

/// A wallet dispatch action that an authorized module or signer may execute.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WalletAction {
    /// Transfer `amount` of `asset` to `to`.
    Transfer {
        to: Address,
        asset: Address,
        amount: i128,
    },
    /// Withdraw `amount` of `asset` back to the wallet owner.
    Withdraw { asset: Address, amount: i128 },
    /// Freeze the wallet, blocking all outbound movement.
    Freeze,
    /// Unfreeze the wallet back to Active.
    Unfreeze,
    /// Pause the wallet temporarily.
    Pause,
    /// Resume a paused wallet.
    Unpause,
}

/// Stored wallet record. `owner` controls the wallet; `state` gates operations.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WalletData {
    pub owner: Address,
    pub state: ResourceState,
}

#[contract]
pub struct WalletContract;

#[contractimpl]
impl WalletContract {
    /// Initialize the contract with an emergency admin, registry address, and
    /// organization slug. Callable once.
    pub fn initialize(
        env: Env,
        admin: Address,
        registry: Address,
        org: String,
    ) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::AlreadyInitialized);
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::WalletCount, &0u64);
        env.storage().instance().set(&DataKey::Registry, &registry);
        env.storage().instance().set(&DataKey::Org, &org);
        Self::bump_instance(&env);
        Ok(())
    }

    /// Create a new wallet owned by `owner`. Returns the new wallet id.
    pub fn create_wallet(env: Env, owner: Address) -> Result<u64, Error> {
        owner.require_auth();
        let mut count: u64 = env
            .storage()
            .instance()
            .get(&DataKey::WalletCount)
            .ok_or(Error::NotInitialized)?;
        count = checked_add(count as i128, 1)? as u64;
        let id = count;
        let data = WalletData {
            owner: owner.clone(),
            state: ResourceState::Active,
        };
        env.storage().persistent().set(&DataKey::Wallet(id), &data);
        Self::bump_wallet(&env, id);
        env.storage().instance().set(&DataKey::WalletCount, &count);
        Self::bump_instance(&env);
        events::wallet_created(&env, id, &owner);
        Ok(id)
    }

    /// Fund a wallet: pulls `amount` of `asset` from `from` into custody and
    /// credits the wallet's internal balance. Requires `from` authorization.
    pub fn deposit(
        env: Env,
        wallet_id: u64,
        from: Address,
        asset: Address,
        amount: i128,
    ) -> Result<(), Error> {
        require_positive_amount(amount)?;
        from.require_auth();
        let wallet = Self::load_wallet(&env, wallet_id)?;
        // Deposits are refused into archived wallets; other states may receive.
        if wallet.state == ResourceState::Archived {
            return Err(Error::WalletArchived);
        }
        // Move real tokens into the contract's custody, then credit internally.
        token::TokenClient::new(&env, &asset).transfer(
            &from,
            &env.current_contract_address(),
            &amount,
        );
        Self::credit(&env, wallet_id, &asset, amount)?;
        env.events().publish(
            (symbol_short!("wallet"), symbol_short!("deposit")),
            (wallet_id, asset, amount),
        );
        Ok(())
    }

    /// Pay `amount` of `asset` from a wallet to an arbitrary recipient. Only the
    /// wallet owner may call, and only while the wallet is `Active`.
    pub fn transfer(
        env: Env,
        caller: Address,
        wallet_id: u64,
        to: Address,
        asset: Address,
        amount: i128,
    ) -> Result<(), Error> {
        require_positive_amount(amount)?;
        let wallet = Self::require_owner(&env, wallet_id, &caller)?;
        Self::require_active(&wallet)?;
        Self::debit(&env, wallet_id, &asset, amount)?;
        token::TokenClient::new(&env, &asset).transfer(
            &env.current_contract_address(),
            &to,
            &amount,
        );
        events::transfer_executed(&env, &env.current_contract_address(), &to, &asset, amount);
        Ok(())
    }

    /// Withdraw `amount` of `asset` from a wallet back to its owner. Only the
    /// owner may call, and only while the wallet is `Active`.
    pub fn withdraw(
        env: Env,
        caller: Address,
        wallet_id: u64,
        asset: Address,
        amount: i128,
    ) -> Result<(), Error> {
        require_positive_amount(amount)?;
        let wallet = Self::require_owner(&env, wallet_id, &caller)?;
        Self::require_active(&wallet)?;
        Self::debit(&env, wallet_id, &asset, amount)?;
        token::TokenClient::new(&env, &asset).transfer(
            &env.current_contract_address(),
            &wallet.owner,
            &amount,
        );
        env.events().publish(
            (symbol_short!("wallet"), symbol_short!("withdraw")),
            (wallet_id, asset, amount),
        );
        Ok(())
    }

    /// Freeze a wallet (owner or admin). Blocks all outbound movement.
    pub fn freeze(env: Env, caller: Address, wallet_id: u64) -> Result<(), Error> {
        let mut wallet = Self::require_owner_or_admin(&env, wallet_id, &caller)?;
        if wallet.state == ResourceState::Archived {
            return Err(Error::WalletArchived);
        }
        wallet.state = ResourceState::Frozen;
        Self::store_wallet(&env, wallet_id, &wallet);
        events::wallet_frozen(&env, wallet_id, &caller);
        Ok(())
    }

    /// Unfreeze a wallet back to `Active` (owner or admin).
    pub fn unfreeze(env: Env, caller: Address, wallet_id: u64) -> Result<(), Error> {
        let mut wallet = Self::require_owner_or_admin(&env, wallet_id, &caller)?;
        if wallet.state != ResourceState::Frozen {
            return Err(Error::InvalidState);
        }
        wallet.state = ResourceState::Active;
        Self::store_wallet(&env, wallet_id, &wallet);
        Self::emit_state(&env, wallet_id, symbol_short!("unfrozen"));
        Ok(())
    }

    /// Pause a wallet (owner only). Temporarily blocks outbound movement.
    pub fn pause(env: Env, caller: Address, wallet_id: u64) -> Result<(), Error> {
        let mut wallet = Self::require_owner(&env, wallet_id, &caller)?;
        if wallet.state != ResourceState::Active {
            return Err(Error::InvalidState);
        }
        wallet.state = ResourceState::Paused;
        Self::store_wallet(&env, wallet_id, &wallet);
        Self::emit_state(&env, wallet_id, symbol_short!("paused"));
        Ok(())
    }

    /// Resume a paused wallet (owner only).
    pub fn unpause(env: Env, caller: Address, wallet_id: u64) -> Result<(), Error> {
        let mut wallet = Self::require_owner(&env, wallet_id, &caller)?;
        if wallet.state != ResourceState::Paused {
            return Err(Error::InvalidState);
        }
        wallet.state = ResourceState::Active;
        Self::store_wallet(&env, wallet_id, &wallet);
        Self::emit_state(&env, wallet_id, symbol_short!("unpaused"));
        Ok(())
    }

    /// Archive a wallet (owner only). Terminal state; no further transactions.
    pub fn archive(env: Env, caller: Address, wallet_id: u64) -> Result<(), Error> {
        let mut wallet = Self::require_owner(&env, wallet_id, &caller)?;
        if wallet.state == ResourceState::Archived {
            return Err(Error::WalletArchived);
        }
        wallet.state = ResourceState::Archived;
        Self::store_wallet(&env, wallet_id, &wallet);
        Self::emit_state(&env, wallet_id, symbol_short!("archived"));
        Ok(())
    }

    /// Dispatch a wallet operation from an authorized caller.
    ///
    /// The `caller` must be one of:
    /// - the wallet's **owner** (verified against on-chain wallet state), or
    /// - a **registered module** for this wallet's organization, confirmed via
    ///   the on-chain registry contract.
    ///
    /// This is the primary entrypoint for organizational modules (multisig,
    /// treasury, policy, etc.) to execute wallet operations on behalf of the
    /// organization. Direct owner calls to `transfer` / `freeze` etc. remain
    /// available for backwards compatibility.
    ///
    /// # Errors
    /// - [`Error::UnauthorizedDispatch`] if the caller is neither the owner nor
    ///   a registered module.
    /// - Propagates any error from the underlying wallet operation.
    pub fn dispatch(
        env: Env,
        caller: Address,
        wallet_id: u64,
        action: WalletAction,
    ) -> Result<(), Error> {
        caller.require_auth();

        // Authorize: caller must be the wallet owner, the wallet admin, or a
        // registered module for this wallet's organization.
        Self::require_registered_caller(&env, &caller, wallet_id)?;

        Self::execute_dispatch(&env, wallet_id, &action)
    }

    // --- views ---

    /// Read a wallet's owner + state.
    pub fn get_wallet(env: Env, wallet_id: u64) -> Result<WalletData, Error> {
        Self::load_wallet(&env, wallet_id)
    }

    /// Read a wallet's internal balance for an asset (0 if none recorded).
    pub fn balance(env: Env, wallet_id: u64, asset: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::Balance(wallet_id, asset))
            .unwrap_or(0)
    }

    // --- internal helpers ---

    fn load_wallet(env: &Env, id: u64) -> Result<WalletData, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Wallet(id))
            .ok_or(Error::NotFound)
    }

    fn store_wallet(env: &Env, id: u64, data: &WalletData) {
        env.storage().persistent().set(&DataKey::Wallet(id), data);
        Self::bump_wallet(env, id);
    }

    fn require_owner(env: &Env, id: u64, caller: &Address) -> Result<WalletData, Error> {
        caller.require_auth();
        let wallet = Self::load_wallet(env, id)?;
        if &wallet.owner != caller {
            return Err(Error::Unauthorized);
        }
        Ok(wallet)
    }

    fn require_owner_or_admin(env: &Env, id: u64, caller: &Address) -> Result<WalletData, Error> {
        caller.require_auth();
        let wallet = Self::load_wallet(env, id)?;
        let admin: Option<Address> = env.storage().instance().get(&DataKey::Admin);
        let is_admin = admin.map(|a| &a == caller).unwrap_or(false);
        if &wallet.owner != caller && !is_admin {
            return Err(Error::Unauthorized);
        }
        Ok(wallet)
    }

    fn require_active(wallet: &WalletData) -> Result<(), Error> {
        match wallet.state {
            ResourceState::Active => Ok(()),
            ResourceState::Frozen => Err(Error::WalletFrozen),
            ResourceState::Paused => Err(Error::WalletPaused),
            ResourceState::Archived => Err(Error::WalletArchived),
        }
    }

    /// Verify that `caller` is authorized to dispatch operations on this wallet.
    ///
    /// Authorization succeeds when **any** of the following hold:
    /// 1. `caller` is the wallet's registered owner.
    /// 2. `caller` is the contract-level admin.
    /// 3. `caller` is a registered module address for this wallet's organization
    ///    in the on-chain registry.
    ///
    /// Returns [`Error::UnauthorizedDispatch`] when none of the above apply.
    fn require_registered_caller(
        env: &Env,
        caller: &Address,
        wallet_id: u64,
    ) -> Result<(), Error> {
        // Fast path: owner or admin — no cross-contract call needed.
        let wallet = Self::load_wallet(env, wallet_id)?;
        let is_owner = wallet.owner == *caller;
        let admin: Option<Address> = env.storage().instance().get(&DataKey::Admin);
        let is_admin = admin.map(|a| &a == caller).unwrap_or(false);

        if is_owner || is_admin {
            return Ok(());
        }

        // Registry path: verify caller is a registered module for this org.
        let registry_addr: Address = env
            .storage()
            .instance()
            .get(&DataKey::Registry)
            .ok_or(Error::NotInitialized)?;
        let org: String = env
            .storage()
            .instance()
            .get(&DataKey::Org)
            .ok_or(Error::NotInitialized)?;

        let registry = RegistryClient::new(env, &registry_addr);

        // Try each known module kind — if the caller is registered as ANY
        // module for this org, it is authorized to dispatch wallet operations.
        let module_kinds = [
            ModuleKind::Multisig,
            ModuleKind::Treasury,
            ModuleKind::Policy,
            ModuleKind::Budget,
            ModuleKind::Proposal,
            ModuleKind::Escrow,
        ];

        for kind in module_kinds.iter() {
            match registry.lookup(&org.clone(), kind) {
                Ok(addr) if addr == *caller => return Ok(()),
                _ => continue,
            }
        }

        Err(Error::UnauthorizedDispatch)
    }

    /// Execute a dispatch action against the specified wallet. The caller has
    /// already been authorized by [`Self::require_registered_caller`].
    fn execute_dispatch(
        env: &Env,
        wallet_id: u64,
        action: &WalletAction,
    ) -> Result<(), Error> {
        match action {
            WalletAction::Transfer { to, asset, amount } => {
                require_positive_amount(*amount)?;
                let wallet = Self::load_wallet(env, wallet_id)?;
                Self::require_active(&wallet)?;
                Self::debit(env, wallet_id, asset, *amount)?;
                token::TokenClient::new(env, asset).transfer(
                    &env.current_contract_address(),
                    to,
                    amount,
                );
                events::transfer_executed(
                    env,
                    &env.current_contract_address(),
                    to,
                    asset,
                    *amount,
                );
            }
            WalletAction::Withdraw { asset, amount } => {
                require_positive_amount(*amount)?;
                let wallet = Self::load_wallet(env, wallet_id)?;
                Self::require_active(&wallet)?;
                Self::debit(env, wallet_id, asset, *amount)?;
                token::TokenClient::new(env, asset).transfer(
                    &env.current_contract_address(),
                    &wallet.owner,
                    amount,
                );
                env.events().publish(
                    (symbol_short!("wallet"), symbol_short!("withdraw")),
                    (wallet_id, asset, *amount),
                );
            }
            WalletAction::Freeze => {
                let mut wallet = Self::load_wallet(env, wallet_id)?;
                if wallet.state == ResourceState::Archived {
                    return Err(Error::WalletArchived);
                }
                wallet.state = ResourceState::Frozen;
                Self::store_wallet(env, wallet_id, &wallet);
                events::wallet_frozen(env, wallet_id, &env.current_contract_address());
            }
            WalletAction::Unfreeze => {
                let mut wallet = Self::load_wallet(env, wallet_id)?;
                if wallet.state != ResourceState::Frozen {
                    return Err(Error::InvalidState);
                }
                wallet.state = ResourceState::Active;
                Self::store_wallet(env, wallet_id, &wallet);
                Self::emit_state(env, wallet_id, symbol_short!("unfrozen"));
            }
            WalletAction::Pause => {
                let mut wallet = Self::load_wallet(env, wallet_id)?;
                if wallet.state != ResourceState::Active {
                    return Err(Error::InvalidState);
                }
                wallet.state = ResourceState::Paused;
                Self::store_wallet(env, wallet_id, &wallet);
                Self::emit_state(env, wallet_id, symbol_short!("paused"));
            }
            WalletAction::Unpause => {
                let mut wallet = Self::load_wallet(env, wallet_id)?;
                if wallet.state != ResourceState::Paused {
                    return Err(Error::InvalidState);
                }
                wallet.state = ResourceState::Active;
                Self::store_wallet(env, wallet_id, &wallet);
                Self::emit_state(env, wallet_id, symbol_short!("unpaused"));
            }
        }
        Ok(())
    }

    fn credit(env: &Env, id: u64, asset: &Address, amount: i128) -> Result<(), Error> {
        let key = DataKey::Balance(id, asset.clone());
        let current: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        let updated = checked_add(current, amount)?;
        env.storage().persistent().set(&key, &updated);
        env.storage().persistent().extend_ttl(
            &key,
            constants::PERSISTENT_LIFETIME_THRESHOLD,
            constants::PERSISTENT_BUMP_AMOUNT,
        );
        Ok(())
    }

    fn debit(env: &Env, id: u64, asset: &Address, amount: i128) -> Result<(), Error> {
        let key = DataKey::Balance(id, asset.clone());
        let current: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        if current < amount {
            return Err(Error::InsufficientFunds);
        }
        let updated = checked_sub(current, amount)?;
        env.storage().persistent().set(&key, &updated);
        env.storage().persistent().extend_ttl(
            &key,
            constants::PERSISTENT_LIFETIME_THRESHOLD,
            constants::PERSISTENT_BUMP_AMOUNT,
        );
        Ok(())
    }

    fn emit_state(env: &Env, id: u64, action: soroban_sdk::Symbol) {
        env.events().publish((symbol_short!("wallet"), action), id);
    }

    fn bump_wallet(env: &Env, id: u64) {
        env.storage().persistent().extend_ttl(
            &DataKey::Wallet(id),
            constants::PERSISTENT_LIFETIME_THRESHOLD,
            constants::PERSISTENT_BUMP_AMOUNT,
        );
    }

    fn bump_instance(env: &Env) {
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }
}

#[cfg(test)]
mod test;
