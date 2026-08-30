use astroid_shared::errors::Error;

use soroban_sdk::{
    testutils::{Address as _, Events, Ledger},
    Address, BytesN, Env, IntoVal, String, Symbol, Val,
};

use crate::{PolicyContract, PolicyContractClient};

/// Assert that the canonical `ContractEvent` with the given variant symbol was
/// published during the test (single-topic event = the variant name).
fn assert_event(env: &Env, variant: &str) {
    let want: Val = Symbol::new(env, variant).into_val(env);
    let found = env
        .events()
        .all()
        .iter()
        .any(|(_contract_id, topics, _data)| topics.contains(want));
    assert!(found, "expected ContractEvent::{} to be emitted", variant);
}

fn setup<'a>(env: &Env, owner: &Address) -> PolicyContractClient<'a> {
    let id = env.register_contract(None, PolicyContract);
    let client = PolicyContractClient::new(env, &id);
    client.initialize(owner);
    client.register_policy(
        owner,
        &String::from_str(env, "max_txn"),
        &BytesN::from_array(env, &[42; 32]),
        &1_000_000,
        &None,
        &soroban_sdk::vec![env],
        &0,
        &0,
        &0,
    );
    client
}

#[test]
fn allows_spend_below_max() {
    let env = Env::default();
    env.mock_all_auths();
    let owner = Address::generate(&env);
    let p = setup(&env, &owner);
    let asset = Address::generate(&env);
    let recip = Address::generate(&env);
    assert!(p
        .try_check_transfer(&String::from_str(&env, "max_txn"), &asset, &recip, &999_999,)
        .is_ok());
}

#[test]
fn denies_spend_above_max() {
    let env = Env::default();
    env.mock_all_auths();
    let owner = Address::generate(&env);
    let p = setup(&env, &owner);
    let asset = Address::generate(&env);
    let recip = Address::generate(&env);
    let r = p.try_check_transfer(
        &String::from_str(&env, "max_txn"),
        &asset,
        &recip,
        &1_000_001,
    );
    assert!(r.is_err());
}

#[test]
fn allowlist_recipient_enforced() {
    let env = Env::default();
    env.mock_all_auths();
    let owner = Address::generate(&env);
    let allowed = Address::generate(&env);
    let blocked = Address::generate(&env);
    let asset = Address::generate(&env);
    let id = env.register_contract(None, PolicyContract);
    let client = PolicyContractClient::new(&env, &id);
    client.initialize(&owner);
    client.register_policy(
        &owner,
        &String::from_str(&env, "vendor_list"),
        &BytesN::from_array(&env, &[7; 32]),
        &0,
        &Some(allowed.clone()),
        &soroban_sdk::vec![&env],
        &0,
        &0,
        &0,
    );

    assert!(client
        .try_check_transfer(&String::from_str(&env, "vendor_list"), &asset, &allowed, &1,)
        .is_ok());

    assert!(client
        .try_check_transfer(&String::from_str(&env, "vendor_list"), &asset, &blocked, &1,)
        .is_err());
}

/// Register a policy and return a client for whitelist tests.
fn register<'a>(env: &'a Env, owner: &Address, policy_id: &str) -> PolicyContractClient<'a> {
    let id = env.register_contract(None, PolicyContract);
    let client = PolicyContractClient::new(env, &id);
    client.initialize(&owner);
    client.register_policy(
        owner,
        &String::from_str(env, policy_id),
        &BytesN::from_array(env, &[9; 32]),
        &0,
        &None,
        &None,
        &0,
    );
    client
}

#[test]
fn whitelist_allows_listed_and_blocks_unlisted() {
    let env = Env::default();
    env.mock_all_auths();
    let owner = Address::generate(&env);
    let client = register(&env, &owner, "wl");
    let asset = Address::generate(&env);
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let stranger = Address::generate(&env);

    client.set_whitelist_enabled(&owner, &String::from_str(&env, "wl"), &true);
    client.add_whitelist(&owner, &String::from_str(&env, "wl"), &a);
    client.add_whitelist(&owner, &String::from_str(&env, "wl"), &b);

    // Listed recipients pass.
    assert!(client
        .try_check_transfer(&String::from_str(&env, "wl"), &asset, &a, &1)
        .is_ok());
    assert!(client
        .try_check_transfer(&String::from_str(&env, "wl"), &asset, &b, &1)
        .is_ok());
    // Unlisted recipient is blocked.
    assert!(client
        .try_check_transfer(&String::from_str(&env, "wl"), &asset, &stranger, &1)
        .is_err());
}

#[test]
fn empty_whitelist_denies_all_recipients() {
    let env = Env::default();
    env.mock_all_auths();
    let owner = Address::generate(&env);
    let client = register(&env, &owner, "wl_empty");
    let asset = Address::generate(&env);
    let anyone = Address::generate(&env);

    client.set_whitelist_enabled(&owner, &String::from_str(&env, "wl_empty"), &true);
    // Whitelist mode active with no entries — fail closed, every recipient denied.
    assert!(client
        .try_check_transfer(&String::from_str(&env, "wl_empty"), &asset, &anyone, &1)
        .is_err());
}

#[test]
fn whitelist_removal_blocks_previously_allowed() {
    let env = Env::default();
    env.mock_all_auths();
    let owner = Address::generate(&env);
    let client = register(&env, &owner, "wl_rem");
    let asset = Address::generate(&env);
    let a = Address::generate(&env);

    client.set_whitelist_enabled(&owner, &String::from_str(&env, "wl_rem"), &true);
    client.add_whitelist(&owner, &String::from_str(&env, "wl_rem"), &a);
    assert!(client
        .try_check_transfer(&String::from_str(&env, "wl_rem"), &asset, &a, &1)
        .is_ok());

    client.remove_whitelist(&owner, &String::from_str(&env, "wl_rem"), &a);
    // Removing an absent entry fails with NotFound.
    assert!(client
        .try_remove_whitelist(&owner, &String::from_str(&env, "wl_rem"), &a)
        .is_err());
    // The previously allowed recipient is now blocked.
    assert!(client
        .try_check_transfer(&String::from_str(&env, "wl_rem"), &asset, &a, &1)
        .is_err());
}

#[test]
fn whitelist_disabled_allows_any_recipient() {
    let env = Env::default();
    env.mock_all_auths();
    let owner = Address::generate(&env);
    let client = register(&env, &owner, "wl_off");
    let asset = Address::generate(&env);
    let anyone = Address::generate(&env);

    // Default state: whitelist mode off, anyone passes.
    assert!(client
        .try_check_transfer(&String::from_str(&env, "wl_off"), &asset, &anyone, &1)
        .is_ok());
    // Toggle off again after enabling also opens the gate.
    client.set_whitelist_enabled(&owner, &String::from_str(&env, "wl_off"), &true);
    client.set_whitelist_enabled(&owner, &String::from_str(&env, "wl_off"), &false);
    assert!(client
        .try_check_transfer(&String::from_str(&env, "wl_off"), &asset, &anyone, &1)
        .is_ok());
}

#[test]
fn non_owner_cannot_manage_whitelist() {
    let env = Env::default();
    env.mock_all_auths();
    let owner = Address::generate(&env);
    let intruder = Address::generate(&env);
    let client = register(&env, &owner, "wl_auth");
    let pid = String::from_str(&env, "wl_auth");
    let target = Address::generate(&env);

    assert!(client
        .try_set_whitelist_enabled(&intruder, &pid, &true)
        .is_err());
    assert!(client.try_add_whitelist(&intruder, &pid, &target).is_err());
    assert!(client
        .try_remove_whitelist(&intruder, &pid, &target)
        .is_err());
}

#[test]
fn duplicate_whitelist_add_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let owner = Address::generate(&env);
    let client = register(&env, &owner, "wl_dup");
    let pid = String::from_str(&env, "wl_dup");
    let target = Address::generate(&env);

    client.add_whitelist(&owner, &pid, &target);
    assert!(client.try_add_whitelist(&owner, &pid, &target).is_err());
}

#[test]
fn disable_denies_everything() {
    let env = Env::default();
    env.mock_all_auths();
    let owner = Address::generate(&env);
    let p = setup(&env, &owner);
    let asset = Address::generate(&env);
    p.set_enabled(&owner, &String::from_str(&env, "max_txn"), &false);
    assert!(p
        .try_check_transfer(
            &String::from_str(&env, "max_txn"),
            &asset,
            &Address::generate(&env),
            &1,
        )
        .is_err());
}

#[test]
fn test_asset_and_window_restrictions() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let contract_id = env.register_contract(None, PolicyContract);
    let client = PolicyContractClient::new(&env, &contract_id);

    client.initialize(&admin);

    let p_id = String::from_str(&env, "p2");
    let asset1 = Address::generate(&env);
    let asset2 = Address::generate(&env);

    // time window: 100 to 200
    client.register_policy(
        &admin,
        &p_id,
        &BytesN::from_array(&env, &[0; 32]),
        &0,
        &None,
        &soroban_sdk::vec![&env, asset1.clone()],
        &100,
        &200,
        &0,
    );

    let recipient = Address::generate(&env);

    // Too early
    env.ledger().set_timestamp(50);
    assert_eq!(
        client.try_check_transfer(&p_id, &asset1, &recipient, &100),
        Err(Ok(Error::PolicyDenied))
    );

    // Too late
    env.ledger().set_timestamp(250);
    assert_eq!(
        client.try_check_transfer(&p_id, &asset1, &recipient, &100),
        Err(Ok(Error::PolicyDenied))
    );

    // In window, but wrong asset
    env.ledger().set_timestamp(150);
    assert_eq!(
        client.try_check_transfer(&p_id, &asset2, &recipient, &100),
        Err(Ok(Error::AssetNotWhitelisted))
    );

    // Correct asset and in window
    assert_eq!(
        client.try_check_transfer(&p_id, &asset1, &recipient, &100),
        Ok(Ok(()))
    );
}

#[test]
fn standard_policy_violation_event_emitted() {
    let env = Env::default();
    env.mock_all_auths();
    let owner = Address::generate(&env);
    let p = setup(&env, &owner);
    let asset = Address::generate(&env);
    let recip = Address::generate(&env);
    let _ = p.try_check_transfer(
        &String::from_str(&env, "max_txn"),
        &asset,
        &recip,
        &1_000_001,
    );
    assert_event(&env, "PolicyViolation");
}

// --- Pause tests ---

#[test]
fn pause_blocks_evaluation_and_unpause_resumes() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1_000);
    let owner = Address::generate(&env);
    let p = setup(&env, &owner);
    let asset = Address::generate(&env);
    let recip = Address::generate(&env);

    assert!(p
        .try_check_transfer(&String::from_str(&env, "max_txn"), &asset, &recip, &1,)
        .is_ok());
    assert!(!p.paused());

    p.pause(&owner, &500);
    assert!(p.paused());
    assert!(p
        .try_check_transfer(&String::from_str(&env, "max_txn"), &asset, &recip, &1,)
        .is_err());

    env.ledger().set_timestamp(1_500);
    assert!(!p.paused());
    assert!(p
        .try_check_transfer(&String::from_str(&env, "max_txn"), &asset, &recip, &1,)
        .is_ok());
}

#[test]
fn indefinite_pause_requires_unpause() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1_000);
    let owner = Address::generate(&env);
    let p = setup(&env, &owner);
    let asset = Address::generate(&env);
    let recip = Address::generate(&env);

    p.pause(&owner, &0);
    assert!(p.paused());
    assert!(p
        .try_check_transfer(&String::from_str(&env, "max_txn"), &asset, &recip, &1,)
        .is_err());

    env.ledger().set_timestamp(1_000_000);
    assert!(p.paused());

    p.unpause(&owner);
    assert!(!p.paused());
    assert!(p
        .try_check_transfer(&String::from_str(&env, "max_txn"), &asset, &recip, &1,)
        .is_ok());
}

#[test]
fn pause_duration_cap_enforced() {
    let env = Env::default();
    env.mock_all_auths();
    let owner = Address::generate(&env);
    let p = setup(&env, &owner);
    let res = p.try_pause(&owner, &(2_592_000 + 1));
    assert!(res.is_err());
    assert!(!p.paused());
}

#[test]
fn only_admin_can_pause() {
    let env = Env::default();
    env.mock_all_auths();
    let owner = Address::generate(&env);
    let intruder = Address::generate(&env);
    let p = setup(&env, &owner);
    let res = p.try_pause(&intruder, &100);
    assert!(res.is_err());
    assert!(!p.paused());
}

// --- Merchant blacklist tests ---

#[test]
fn merchant_blacklist_blocks_transfers() {
    let env = Env::default();
    env.mock_all_auths();
    let owner = Address::generate(&env);
    let p = setup(&env, &owner);
    let asset = Address::generate(&env);
    let blocked_merchant = Address::generate(&env);

    p.add_merchant_blacklist(
        &owner,
        &String::from_str(&env, "max_txn"),
        &blocked_merchant,
    );

    let result = p.try_check_transfer(
        &String::from_str(&env, "max_txn"),
        &asset,
        &blocked_merchant,
        &100,
    );
    assert!(result.is_err());

    let safe_merchant = Address::generate(&env);
    assert!(p
        .try_check_transfer(
            &String::from_str(&env, "max_txn"),
            &asset,
            &safe_merchant,
            &100,
        )
        .is_ok());
}

#[test]
fn merchant_blacklist_removal_allows_transfers() {
    let env = Env::default();
    env.mock_all_auths();
    let owner = Address::generate(&env);
    let p = setup(&env, &owner);
    let asset = Address::generate(&env);
    let merchant = Address::generate(&env);

    p.add_merchant_blacklist(&owner, &String::from_str(&env, "max_txn"), &merchant);
    assert!(p
        .try_check_transfer(&String::from_str(&env, "max_txn"), &asset, &merchant, &100,)
        .is_err());

    p.remove_merchant_blacklist(&owner, &String::from_str(&env, "max_txn"), &merchant);
    assert!(p
        .try_check_transfer(&String::from_str(&env, "max_txn"), &asset, &merchant, &100,)
        .is_ok());
}

#[test]
fn merchant_blacklist_unauthorized_add_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let owner = Address::generate(&env);
    let unauthorized = Address::generate(&env);
    let p = setup(&env, &owner);
    let merchant = Address::generate(&env);

    let result =
        p.try_add_merchant_blacklist(&unauthorized, &String::from_str(&env, "max_txn"), &merchant);
    assert!(result.is_err());
}

#[test]
fn merchant_blacklist_duplicate_add_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let owner = Address::generate(&env);
    let p = setup(&env, &owner);
    let merchant = Address::generate(&env);

    p.add_merchant_blacklist(&owner, &String::from_str(&env, "max_txn"), &merchant);
    let result =
        p.try_add_merchant_blacklist(&owner, &String::from_str(&env, "max_txn"), &merchant);
    assert!(result.is_err());
}

#[test]
fn merchant_blacklist_nonexistent_remove_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let owner = Address::generate(&env);
    let p = setup(&env, &owner);
    let merchant = Address::generate(&env);

    let result =
        p.try_remove_merchant_blacklist(&owner, &String::from_str(&env, "max_txn"), &merchant);
    assert!(result.is_err());
}

#[test]
fn merchant_blocked_event_emitted() {
    let env = Env::default();
    env.mock_all_auths();
    let owner = Address::generate(&env);
    let p = setup(&env, &owner);
    let asset = Address::generate(&env);
    let blocked_merchant = Address::generate(&env);

    p.add_merchant_blacklist(
        &owner,
        &String::from_str(&env, "max_txn"),
        &blocked_merchant,
    );

    let _ = p.try_check_transfer(
        &String::from_str(&env, "max_txn"),
        &asset,
        &blocked_merchant,
        &100,
    );
    assert_event(&env, "PolicyViolation");
}

// --- Category blacklist tests ---

#[test]
fn category_blacklist_blocks_categories() {
    let env = Env::default();
    env.mock_all_auths();
    let owner = Address::generate(&env);
    let p = setup(&env, &owner);

    p.add_category_blacklist(
        &owner,
        &String::from_str(&env, "max_txn"),
        &String::from_str(&env, "gambling"),
    );

    let result = p.try_check_category(
        &String::from_str(&env, "max_txn"),
        &String::from_str(&env, "gambling"),
    );
    assert!(result.is_err());

    assert!(p
        .try_check_category(
            &String::from_str(&env, "max_txn"),
            &String::from_str(&env, "groceries"),
        )
        .is_ok());

    assert!(p
        .try_check_category(
            &String::from_str(&env, "max_txn"),
            &String::from_str(&env, ""),
        )
        .is_ok());
}

#[test]
fn category_blacklist_removal_allows_categories() {
    let env = Env::default();
    env.mock_all_auths();
    let owner = Address::generate(&env);
    let p = setup(&env, &owner);

    p.add_category_blacklist(
        &owner,
        &String::from_str(&env, "max_txn"),
        &String::from_str(&env, "gambling"),
    );
    assert!(p
        .try_check_category(
            &String::from_str(&env, "max_txn"),
            &String::from_str(&env, "gambling"),
        )
        .is_err());

    p.remove_category_blacklist(
        &owner,
        &String::from_str(&env, "max_txn"),
        &String::from_str(&env, "gambling"),
    );
    assert!(p
        .try_check_category(
            &String::from_str(&env, "max_txn"),
            &String::from_str(&env, "gambling"),
        )
        .is_ok());
}

#[test]
fn category_blacklist_unauthorized_add_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let owner = Address::generate(&env);
    let unauthorized = Address::generate(&env);
    let p = setup(&env, &owner);

    let result = p.try_add_category_blacklist(
        &unauthorized,
        &String::from_str(&env, "max_txn"),
        &String::from_str(&env, "gambling"),
    );
    assert!(result.is_err());
}

#[test]
fn category_blacklist_duplicate_add_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let owner = Address::generate(&env);
    let p = setup(&env, &owner);

    p.add_category_blacklist(
        &owner,
        &String::from_str(&env, "max_txn"),
        &String::from_str(&env, "gambling"),
    );
    let result = p.try_add_category_blacklist(
        &owner,
        &String::from_str(&env, "max_txn"),
        &String::from_str(&env, "gambling"),
    );
    assert!(result.is_err());
}

#[test]
fn category_blacklist_nonexistent_remove_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let owner = Address::generate(&env);
    let p = setup(&env, &owner);

    let result = p.try_remove_category_blacklist(
        &owner,
        &String::from_str(&env, "max_txn"),
        &String::from_str(&env, "gambling"),
    );
    assert!(result.is_err());
}

#[test]
fn category_blacklist_empty_category_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let owner = Address::generate(&env);
    let p = setup(&env, &owner);

    let result = p.try_add_category_blacklist(
        &owner,
        &String::from_str(&env, "max_txn"),
        &String::from_str(&env, ""),
    );
    assert!(result.is_err());
}

#[test]
fn category_restricted_event_emitted() {
    let env = Env::default();
    env.mock_all_auths();
    let owner = Address::generate(&env);
    let p = setup(&env, &owner);

    p.add_category_blacklist(
        &owner,
        &String::from_str(&env, "max_txn"),
        &String::from_str(&env, "gambling"),
    );

    let _ = p.try_check_category(
        &String::from_str(&env, "max_txn"),
        &String::from_str(&env, "gambling"),
    );
    assert_event(&env, "PolicyViolation");
}

// --- Blocklist tests ---

#[test]
fn blocklist_blocks_transfers() {
    let env = Env::default();
    env.mock_all_auths();
    let owner = Address::generate(&env);
    let p = setup(&env, &owner);
    let asset = Address::generate(&env);
    let blocked = Address::generate(&env);
    let safe = Address::generate(&env);

    p.add_to_blocklist(&owner, &String::from_str(&env, "max_txn"), &blocked);

    let result = p.try_check_transfer(&String::from_str(&env, "max_txn"), &asset, &blocked, &100);
    assert!(result.is_err());

    assert!(p
        .try_check_transfer(&String::from_str(&env, "max_txn"), &asset, &safe, &100,)
        .is_ok());
}

#[test]
fn blocklist_removal_allows_transfers() {
    let env = Env::default();
    env.mock_all_auths();
    let owner = Address::generate(&env);
    let p = setup(&env, &owner);
    let asset = Address::generate(&env);
    let recip = Address::generate(&env);

    p.add_to_blocklist(&owner, &String::from_str(&env, "max_txn"), &recip);
    assert!(p
        .try_check_transfer(&String::from_str(&env, "max_txn"), &asset, &recip, &100,)
        .is_err());

    p.remove_from_blocklist(&owner, &String::from_str(&env, "max_txn"), &recip);
    assert!(p
        .try_check_transfer(&String::from_str(&env, "max_txn"), &asset, &recip, &100,)
        .is_ok());
}

#[test]
fn blocklist_unauthorized_add_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let owner = Address::generate(&env);
    let unauthorized = Address::generate(&env);
    let p = setup(&env, &owner);
    let addr = Address::generate(&env);

    let result = p.try_add_to_blocklist(&unauthorized, &String::from_str(&env, "max_txn"), &addr);
    assert!(result.is_err());
}

#[test]
fn blocklist_duplicate_add_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let owner = Address::generate(&env);
    let p = setup(&env, &owner);
    let addr = Address::generate(&env);

    p.add_to_blocklist(&owner, &String::from_str(&env, "max_txn"), &addr);
    let result = p.try_add_to_blocklist(&owner, &String::from_str(&env, "max_txn"), &addr);
    assert!(result.is_err());
}

#[test]
fn blocklist_nonexistent_remove_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let owner = Address::generate(&env);
    let p = setup(&env, &owner);
    let addr = Address::generate(&env);

    let result = p.try_remove_from_blocklist(&owner, &String::from_str(&env, "max_txn"), &addr);
    assert!(result.is_err());
}

#[test]
fn blocklist_violation_event_emitted() {
    let env = Env::default();
    env.mock_all_auths();
    let owner = Address::generate(&env);
    let p = setup(&env, &owner);
    let asset = Address::generate(&env);
    let blocked = Address::generate(&env);

    p.add_to_blocklist(&owner, &String::from_str(&env, "max_txn"), &blocked);

    let _ = p.try_check_transfer(&String::from_str(&env, "max_txn"), &asset, &blocked, &100);
    assert_event(&env, "PolicyViolation");
}

// --- Asset whitelist tests ---

#[test]
fn asset_whitelist_allows_whitelisted_asset() {
    let env = Env::default();
    env.mock_all_auths();
    let owner = Address::generate(&env);
    let p = setup(&env, &owner);
    let asset = Address::generate(&env);

    p.set_asset_whitelist_enabled(&owner, &String::from_str(&env, "max_txn"), &true);
    p.add_asset_to_whitelist(&owner, &String::from_str(&env, "max_txn"), &asset);

    let recip = Address::generate(&env);
    assert!(p
        .try_check_transfer(&String::from_str(&env, "max_txn"), &asset, &recip, &100,)
        .is_ok());
}

#[test]
fn asset_whitelist_add_remove_roundtrip() {
    let env = Env::default();
    env.mock_all_auths();
    let owner = Address::generate(&env);
    let p = setup(&env, &owner);
    let asset = Address::generate(&env);

    p.set_asset_whitelist_enabled(&owner, &String::from_str(&env, "max_txn"), &true);
    p.add_asset_to_whitelist(&owner, &String::from_str(&env, "max_txn"), &asset);

    p.remove_asset_from_whitelist(&owner, &String::from_str(&env, "max_txn"), &asset);

    assert!(p
        .try_validate_asset(&String::from_str(&env, "max_txn"), &asset)
        .is_err());
}

#[test]
fn asset_whitelist_unauthorized_add_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let owner = Address::generate(&env);
    let unauthorized = Address::generate(&env);
    let p = setup(&env, &owner);
    let addr = Address::generate(&env);

    let result = p.try_add_to_blocklist(&unauthorized, &String::from_str(&env, "max_txn"), &addr);
    assert!(result.is_err());
}

#[test]
fn asset_whitelist_duplicate_add_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let owner = Address::generate(&env);
    let p = setup(&env, &owner);
    let asset = Address::generate(&env);

    p.add_asset_to_whitelist(&owner, &String::from_str(&env, "max_txn"), &asset);

    let result = p.try_add_asset_to_whitelist(&owner, &String::from_str(&env, "max_txn"), &asset);
    assert!(result.is_err());
}

#[test]
fn asset_whitelist_nonexistent_remove_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let owner = Address::generate(&env);
    let p = setup(&env, &owner);
    let asset = Address::generate(&env);

    let result =
        p.try_remove_asset_from_whitelist(&owner, &String::from_str(&env, "max_txn"), &asset);
    assert!(result.is_err());
}

#[test]
fn asset_whitelist_empty_default_passes_validate() {
    let env = Env::default();
    env.mock_all_auths();
    let owner = Address::generate(&env);
    let p = setup(&env, &owner);
    let asset = Address::generate(&env);

    assert!(p
        .try_validate_asset(&String::from_str(&env, "max_txn"), &asset)
        .is_ok());
}

#[test]
fn asset_whitelist_violation_event_emitted() {
    let env = Env::default();
    env.mock_all_auths();
    let owner = Address::generate(&env);
    let p = setup(&env, &owner);
    let asset = Address::generate(&env);
    let recip = Address::generate(&env);

    p.set_asset_whitelist_enabled(&owner, &String::from_str(&env, "max_txn"), &true);

    let _ = p.try_check_transfer(&String::from_str(&env, "max_txn"), &asset, &recip, &100);
    assert_event(&env, "PolicyViolation");
}
