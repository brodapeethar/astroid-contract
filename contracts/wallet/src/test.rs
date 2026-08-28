#![cfg(test)]
extern crate std;

use crate::{WalletAction, WalletContract, WalletContractClient};
use astroid_shared::errors::Error;
use astroid_shared::types::{ModuleKind, ResourceState};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{symbol_short, token, Address, Env, String};

struct Harness {
    env: Env,
    client: WalletContractClient<'static>,
    token: Address,
    registry_addr: Address,
    org: String,
    admin: Address,
}

/// A minimal mock registry that stores module addresses in instance storage.
/// Only the `lookup` function is needed for wallet authorization checks.
#[soroban_sdk::contract]
pub struct MockRegistry;

#[soroban_sdk::contractimpl]
impl MockRegistry {
    pub fn __constructor(_env: Env) {}

    /// Store a module address: key = (org, kind), value = address.
    pub fn set_module(
        env: Env,
        org: String,
        kind: ModuleKind,
        address: Address,
    ) {
        use soroban_sdk::contracttype;
        #[contracttype]
        enum Key {
            Module(String, ModuleKind),
        }
        env.storage()
            .instance()
            .set(&Key::Module(org, kind), &address);
    }

    /// RegistryInterface::lookup
    pub fn lookup(
        env: Env,
        org: String,
        kind: ModuleKind,
    ) -> Result<Address, Error> {
        use soroban_sdk::contracttype;
        #[contracttype]
        enum Key {
            Module(String, ModuleKind),
        }
        env.storage()
            .instance()
            .get(&Key::Module(org, kind))
            .ok_or(Error::NotFound)
    }

    /// RegistryInterface::verify_owner — not used by wallet but required by the
    /// trait.
    pub fn verify_owner(
        _env: Env,
        _org: String,
        _owner: Address,
    ) -> Result<bool, Error> {
        Ok(false)
    }
}

fn setup() -> Harness {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);

    // Deploy the mock registry.
    let registry_id = env.register_contract(None, MockRegistry);
    let registry_addr = registry_id.clone();

    let org = String::from_str(&env, "test-org");

    // Deploy the wallet contract.
    let contract_id = env.register_contract(None, WalletContract);
    let client = WalletContractClient::new(&env, &contract_id);
    client.initialize(&admin, &registry_addr, &org);

    // A test SAC token whose admin can mint funds to users.
    let token_admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    Harness {
        env,
        client,
        token,
        registry_addr,
        org,
        admin,
    }
}

fn mint(h: &Harness, to: &Address, amount: i128) {
    let sac = token::StellarAssetClient::new(&h.env, &h.token);
    sac.mint(to, &amount);
}

fn token_balance(h: &Harness, who: &Address) -> i128 {
    token::TokenClient::new(&h.env, &h.token).balance(who)
}

/// Helper to register a module address in the mock registry.
fn register_mock_module(h: &Harness, kind: ModuleKind, addr: &Address) {
    // Call the mock registry's set_module directly via the contract client.
    // We need the raw contract client for the mock.
    #[soroban_sdk::contractclient(name = "MockRegistryClient")]
    pub trait MockRegistryInterface {
        fn set_module(env: Env, org: String, kind: ModuleKind, address: Address);
        fn lookup(env: Env, org: String, kind: ModuleKind) -> Result<Address, Error>;
    }
    let mock_client = MockRegistryClient::new(&h.env, &h.registry_addr);
    mock_client.set_module(&h.org, &kind, addr);
}

// ---------------------------------------------------------------------------
// Existing tests (updated for new initialize signature)
// ---------------------------------------------------------------------------

#[test]
fn create_wallet_starts_active() {
    let h = setup();
    let owner = Address::generate(&h.env);
    let id = h.client.create_wallet(&owner);
    let w = h.client.get_wallet(&id);
    assert_eq!(w.owner, owner);
    assert_eq!(w.state, ResourceState::Active);
}

#[test]
fn deposit_credits_internal_balance_and_moves_tokens() {
    let h = setup();
    let owner = Address::generate(&h.env);
    let id = h.client.create_wallet(&owner);
    mint(&h, &owner, 1_000);

    h.client.deposit(&id, &owner, &h.token, &400);
    assert_eq!(h.client.balance(&id, &h.token), 400);
    assert_eq!(token_balance(&h, &owner), 600);
}

#[test]
fn transfer_moves_value_and_debits_wallet() {
    let h = setup();
    let owner = Address::generate(&h.env);
    let recipient = Address::generate(&h.env);
    let id = h.client.create_wallet(&owner);
    mint(&h, &owner, 1_000);
    h.client.deposit(&id, &owner, &h.token, &1_000);

    h.client.transfer(&owner, &id, &recipient, &h.token, &250);
    assert_eq!(h.client.balance(&id, &h.token), 750);
    assert_eq!(token_balance(&h, &recipient), 250);
}

#[test]
fn withdraw_returns_to_owner() {
    let h = setup();
    let owner = Address::generate(&h.env);
    let id = h.client.create_wallet(&owner);
    mint(&h, &owner, 1_000);
    h.client.deposit(&id, &owner, &h.token, &1_000);

    h.client.withdraw(&owner, &id, &h.token, &300);
    assert_eq!(h.client.balance(&id, &h.token), 700);
    assert_eq!(token_balance(&h, &owner), 300);
}

#[test]
fn transfer_more_than_balance_fails() {
    let h = setup();
    let owner = Address::generate(&h.env);
    let recipient = Address::generate(&h.env);
    let id = h.client.create_wallet(&owner);
    mint(&h, &owner, 100);
    h.client.deposit(&id, &owner, &h.token, &100);

    let res = h
        .client
        .try_transfer(&owner, &id, &recipient, &h.token, &500);
    assert_eq!(res, Err(Ok(Error::InsufficientFunds)));
}

#[test]
fn non_owner_cannot_transfer() {
    let h = setup();
    let owner = Address::generate(&h.env);
    let intruder = Address::generate(&h.env);
    let recipient = Address::generate(&h.env);
    let id = h.client.create_wallet(&owner);
    mint(&h, &owner, 100);
    h.client.deposit(&id, &owner, &h.token, &100);

    let res = h
        .client
        .try_transfer(&intruder, &id, &recipient, &h.token, &10);
    assert_eq!(res, Err(Ok(Error::Unauthorized)));
}

#[test]
fn transfer_while_frozen_fails_wallet_frozen() {
    let h = setup();
    let owner = Address::generate(&h.env);
    let recipient = Address::generate(&h.env);
    let id = h.client.create_wallet(&owner);
    mint(&h, &owner, 100);
    h.client.deposit(&id, &owner, &h.token, &100);

    h.client.freeze(&owner, &id);
    assert_eq!(h.client.get_wallet(&id).state, ResourceState::Frozen);

    let res = h
        .client
        .try_transfer(&owner, &id, &recipient, &h.token, &10);
    assert_eq!(res, Err(Ok(Error::WalletFrozen)));
}

#[test]
fn admin_can_freeze_then_owner_unfreezes() {
    let h = setup();
    let owner = Address::generate(&h.env);
    let id = h.client.create_wallet(&owner);

    // Owner freezes, then unfreezes back to Active.
    h.client.freeze(&owner, &id);
    h.client.unfreeze(&owner, &id);
    assert_eq!(h.client.get_wallet(&id).state, ResourceState::Active);
}

#[test]
fn transfer_while_paused_fails() {
    let h = setup();
    let owner = Address::generate(&h.env);
    let recipient = Address::generate(&h.env);
    let id = h.client.create_wallet(&owner);
    mint(&h, &owner, 100);
    h.client.deposit(&id, &owner, &h.token, &100);

    h.client.pause(&owner, &id);
    let res = h
        .client
        .try_transfer(&owner, &id, &recipient, &h.token, &10);
    assert_eq!(res, Err(Ok(Error::WalletPaused)));

    h.client.unpause(&owner, &id);
    h.client.transfer(&owner, &id, &recipient, &h.token, &10);
    assert_eq!(token_balance(&h, &recipient), 10);
}

#[test]
fn archived_wallet_rejects_transfer_and_deposit() {
    let h = setup();
    let owner = Address::generate(&h.env);
    let recipient = Address::generate(&h.env);
    let id = h.client.create_wallet(&owner);
    mint(&h, &owner, 100);
    h.client.deposit(&id, &owner, &h.token, &100);

    h.client.archive(&owner, &id);
    assert_eq!(h.client.get_wallet(&id).state, ResourceState::Archived);

    assert_eq!(
        h.client
            .try_transfer(&owner, &id, &recipient, &h.token, &10),
        Err(Ok(Error::WalletArchived))
    );
    mint(&h, &owner, 100);
    assert_eq!(
        h.client.try_deposit(&id, &owner, &h.token, &10),
        Err(Ok(Error::WalletArchived))
    );
}

#[test]
fn zero_amount_transfer_rejected() {
    let h = setup();
    let owner = Address::generate(&h.env);
    let recipient = Address::generate(&h.env);
    let id = h.client.create_wallet(&owner);
    let res = h.client.try_transfer(&owner, &id, &recipient, &h.token, &0);
    assert_eq!(res, Err(Ok(Error::InvalidAmount)));
}

#[test]
fn unknown_wallet_fails_not_found() {
    let h = setup();
    let stranger = Address::generate(&h.env);
    let res = h.client.try_get_wallet(&999);
    assert_eq!(res, Err(Ok(Error::NotFound)));
    let res2 = h.client.try_freeze(&stranger, &999);
    assert_eq!(res2, Err(Ok(Error::NotFound)));
}

// ---------------------------------------------------------------------------
// Dispatch authorization tests
// ---------------------------------------------------------------------------

#[test]
fn dispatch_owner_can_transfer() {
    let h = setup();
    let owner = Address::generate(&h.env);
    let recipient = Address::generate(&h.env);
    let id = h.client.create_wallet(&owner);
    mint(&h, &owner, 1_000);
    h.client.deposit(&id, &owner, &h.token, &1_000);

    let action = WalletAction::Transfer {
        to: recipient.clone(),
        asset: h.token.clone(),
        amount: 250,
    };
    h.client.dispatch(&owner, &id, &action);

    assert_eq!(h.client.balance(&id, &h.token), 750);
    assert_eq!(token_balance(&h, &recipient), 250);
}

#[test]
fn dispatch_owner_can_freeze_and_unfreeze() {
    let h = setup();
    let owner = Address::generate(&h.env);
    let id = h.client.create_wallet(&owner);

    // Freeze via dispatch
    h.client.dispatch(&owner, &id, &WalletAction::Freeze);
    assert_eq!(h.client.get_wallet(&id).state, ResourceState::Frozen);

    // Unfreeze via dispatch
    h.client.dispatch(&owner, &id, &WalletAction::Unfreeze);
    assert_eq!(h.client.get_wallet(&id).state, ResourceState::Active);
}

#[test]
fn dispatch_registered_module_can_transfer() {
    let h = setup();
    let owner = Address::generate(&h.env);
    let module = Address::generate(&h.env); // This will be the "registered" module
    let recipient = Address::generate(&h.env);
    let id = h.client.create_wallet(&owner);
    mint(&h, &owner, 1_000);
    h.client.deposit(&id, &owner, &h.token, &1_000);

    // Register the module as a Multisig module for this org
    register_mock_module(&h, ModuleKind::Multisig, &module);

    let action = WalletAction::Transfer {
        to: recipient.clone(),
        asset: h.token.clone(),
        amount: 500,
    };
    h.client.dispatch(&module, &id, &action);

    assert_eq!(h.client.balance(&id, &h.token), 500);
    assert_eq!(token_balance(&h, &recipient), 500);
}

#[test]
fn dispatch_registered_module_can_freeze() {
    let h = setup();
    let owner = Address::generate(&h.env);
    let module = Address::generate(&h.env);
    let id = h.client.create_wallet(&owner);

    // Register the module as a Treasury module for this org
    register_mock_module(&h, ModuleKind::Treasury, &module);

    h.client.dispatch(&module, &id, &WalletAction::Freeze);
    assert_eq!(h.client.get_wallet(&id).state, ResourceState::Frozen);
}

#[test]
fn dispatch_unregistered_caller_blocked() {
    let h = setup();
    let owner = Address::generate(&h.env);
    let stranger = Address::generate(&h.env);
    let recipient = Address::generate(&h.env);
    let id = h.client.create_wallet(&owner);
    mint(&h, &owner, 1_000);
    h.client.deposit(&id, &owner, &h.token, &1_000);

    // stranger is not the owner and not registered in the registry
    let action = WalletAction::Transfer {
        to: recipient.clone(),
        asset: h.token.clone(),
        amount: 100,
    };
    let res = h.client.try_dispatch(&stranger, &id, &action);
    assert_eq!(res, Err(Ok(Error::UnauthorizedDispatch)));
}

#[test]
fn dispatch_unregistered_cannot_freeze() {
    let h = setup();
    let owner = Address::generate(&h.env);
    let stranger = Address::generate(&h.env);
    let id = h.client.create_wallet(&owner);

    let res = h.client.try_dispatch(&stranger, &id, &WalletAction::Freeze);
    assert_eq!(res, Err(Ok(Error::UnauthorizedDispatch)));
}

#[test]
fn dispatch_unregistered_cannot_withdraw() {
    let h = setup();
    let owner = Address::generate(&h.env);
    let stranger = Address::generate(&h.env);
    let id = h.client.create_wallet(&owner);
    mint(&h, &owner, 1_000);
    h.client.deposit(&id, &owner, &h.token, &1_000);

    let action = WalletAction::Withdraw {
        asset: h.token.clone(),
        amount: 100,
    };
    let res = h.client.try_dispatch(&stranger, &id, &action);
    assert_eq!(res, Err(Ok(Error::UnauthorizedDispatch)));
}

#[test]
fn dispatch_owner_can_pause_and_unpause() {
    let h = setup();
    let owner = Address::generate(&h.env);
    let id = h.client.create_wallet(&owner);

    h.client.dispatch(&owner, &id, &WalletAction::Pause);
    assert_eq!(h.client.get_wallet(&id).state, ResourceState::Paused);

    h.client.dispatch(&owner, &id, &WalletAction::Unpause);
    assert_eq!(h.client.get_wallet(&id).state, ResourceState::Active);
}

#[test]
fn dispatch_registered_module_can_pause() {
    let h = setup();
    let owner = Address::generate(&h.env);
    let module = Address::generate(&h.env);
    let id = h.client.create_wallet(&owner);

    register_mock_module(&h, ModuleKind::Policy, &module);

    h.client.dispatch(&module, &id, &WalletAction::Pause);
    assert_eq!(h.client.get_wallet(&id).state, ResourceState::Paused);

    h.client.dispatch(&module, &id, &WalletAction::Unpause);
    assert_eq!(h.client.get_wallet(&id).state, ResourceState::Active);
}

#[test]
fn dispatch_frozen_wallet_blocks_transfer() {
    let h = setup();
    let owner = Address::generate(&h.env);
    let recipient = Address::generate(&h.env);
    let id = h.client.create_wallet(&owner);
    mint(&h, &owner, 1_000);
    h.client.deposit(&id, &owner, &h.token, &1_000);

    // Freeze the wallet via direct call
    h.client.freeze(&owner, &id);

    // Attempt transfer via dispatch — should fail because wallet is frozen
    let action = WalletAction::Transfer {
        to: recipient.clone(),
        asset: h.token.clone(),
        amount: 100,
    };
    let res = h.client.try_dispatch(&owner, &id, &action);
    assert_eq!(res, Err(Ok(Error::WalletFrozen)));
}

#[test]
fn dispatch_archived_wallet_blocks_freeze() {
    let h = setup();
    let owner = Address::generate(&h.env);
    let id = h.client.create_wallet(&owner);

    h.client.archive(&owner, &id);

    let res = h.client.try_dispatch(&owner, &id, &WalletAction::Freeze);
    assert_eq!(res, Err(Ok(Error::WalletArchived)));
}

#[test]
fn dispatch_zero_amount_transfer_rejected() {
    let h = setup();
    let owner = Address::generate(&h.env);
    let recipient = Address::generate(&h.env);
    let id = h.client.create_wallet(&owner);

    let action = WalletAction::Transfer {
        to: recipient.clone(),
        asset: h.token.clone(),
        amount: 0,
    };
    let res = h.client.try_dispatch(&owner, &id, &action);
    assert_eq!(res, Err(Ok(Error::InvalidAmount)));
}

#[test]
fn dispatch_different_module_kind_registered() {
    let h = setup();
    let owner = Address::generate(&h.env);
    let recipient = Address::generate(&h.env);
    let id = h.client.create_wallet(&owner);
    mint(&h, &owner, 1_000);
    h.client.deposit(&id, &owner, &h.token, &1_000);

    // Register as Escrow module — different kind, same authorization
    let module = Address::generate(&h.env);
    register_mock_module(&h, ModuleKind::Escrow, &module);

    let action = WalletAction::Transfer {
        to: recipient.clone(),
        asset: h.token.clone(),
        amount: 100,
    };
    h.client.dispatch(&module, &id, &action);
    assert_eq!(h.client.balance(&id, &h.token), 900);
}
