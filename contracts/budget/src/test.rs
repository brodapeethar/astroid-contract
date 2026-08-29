#![cfg(test)]
extern crate std;

use crate::{Budget, BudgetContract, BudgetContractClient, Period};
use astroid_shared::errors::Error;
use astroid_shared::types::ResourceState;
use soroban_sdk::testutils::{Address as _, Ledger};
use soroban_sdk::{Address, Env, String};

struct Harness {
    env: Env,
    client: BudgetContractClient<'static>,
    owner: Address,
}

fn setup() -> Harness {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1_000);
    let contract_id = env.register_contract(None, BudgetContract);
    let client = BudgetContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let owner = Address::generate(&env);
    Harness { env, client, owner }
}

fn id(env: &Env, s: &str) -> String {
    String::from_str(env, s)
}

#[test]
fn allocate_creates_active_budget() {
    let h = setup();
    h.client.allocate(
        &h.owner,
        &id(&h.env, "eng"),
        &1_000,
        &Period::None,
        &false,
        &0,
    );
    let b: Budget = h.client.get(&id(&h.env, "eng"));
    assert_eq!(b.limit, 1_000);
    assert_eq!(b.spent, 0);
    assert_eq!(b.state, ResourceState::Active);
    assert!(!b.rollover_enabled);
    assert_eq!(b.rollover_credit, 0);
    assert_eq!(h.client.remaining(&id(&h.env, "eng")), 1_000);
}

#[test]
fn duplicate_allocation_fails() {
    let h = setup();
    h.client.allocate(
        &h.owner,
        &id(&h.env, "eng"),
        &1_000,
        &Period::None,
        &false,
        &0,
    );
    let res = h.client.try_allocate(
        &h.owner,
        &id(&h.env, "eng"),
        &2_000,
        &Period::None,
        &false,
        &0,
    );
    assert_eq!(res, Err(Ok(Error::AlreadyExists)));
}

#[test]
fn consume_reduces_remaining() {
    let h = setup();
    h.client.allocate(
        &h.owner,
        &id(&h.env, "eng"),
        &1_000,
        &Period::None,
        &false,
        &0,
    );
    let rem = h.client.consume(&h.owner, &id(&h.env, "eng"), &400);
    assert_eq!(rem, 600);
    assert_eq!(h.client.remaining(&id(&h.env, "eng")), 600);
    let b: Budget = h.client.get(&id(&h.env, "eng"));
    assert_eq!(b.spent, 400);
}

#[test]
fn over_budget_consume_fails_budget_exceeded() {
    let h = setup();
    h.client.allocate(
        &h.owner,
        &id(&h.env, "eng"),
        &1_000,
        &Period::None,
        &false,
        &0,
    );
    h.client.consume(&h.owner, &id(&h.env, "eng"), &800);
    let res = h.client.try_consume(&h.owner, &id(&h.env, "eng"), &300);
    assert_eq!(res, Err(Ok(Error::BudgetExceeded)));
    // Spend up to the exact limit is allowed.
    let rem = h.client.consume(&h.owner, &id(&h.env, "eng"), &200);
    assert_eq!(rem, 0);
}

#[test]
fn consume_zero_or_negative_rejected() {
    let h = setup();
    h.client.allocate(
        &h.owner,
        &id(&h.env, "eng"),
        &1_000,
        &Period::None,
        &false,
        &0,
    );
    let res = h.client.try_consume(&h.owner, &id(&h.env, "eng"), &0);
    assert_eq!(res, Err(Ok(Error::InvalidAmount)));
    let res = h.client.try_consume(&h.owner, &id(&h.env, "eng"), &-5);
    assert_eq!(res, Err(Ok(Error::InvalidAmount)));
}

#[test]
fn non_owner_cannot_consume() {
    let h = setup();
    h.client.allocate(
        &h.owner,
        &id(&h.env, "eng"),
        &1_000,
        &Period::None,
        &false,
        &0,
    );
    let stranger = Address::generate(&h.env);
    let res = h.client.try_consume(&stranger, &id(&h.env, "eng"), &100);
    assert_eq!(res, Err(Ok(Error::Unauthorized)));
}

#[test]
fn reset_clears_spent() {
    let h = setup();
    h.client.allocate(
        &h.owner,
        &id(&h.env, "eng"),
        &1_000,
        &Period::None,
        &false,
        &0,
    );
    h.client.consume(&h.owner, &id(&h.env, "eng"), &900);
    h.client.reset(&h.owner, &id(&h.env, "eng"));
    assert_eq!(h.client.remaining(&id(&h.env, "eng")), 1_000);
}

#[test]
fn frozen_budget_rejects_consume() {
    let h = setup();
    h.client.allocate(
        &h.owner,
        &id(&h.env, "eng"),
        &1_000,
        &Period::None,
        &false,
        &0,
    );
    h.client.freeze(&h.owner, &id(&h.env, "eng"));
    let res = h.client.try_consume(&h.owner, &id(&h.env, "eng"), &100);
    assert_eq!(res, Err(Ok(Error::BudgetFrozen)));
    // Unfreeze restores spending.
    h.client.unfreeze(&h.owner, &id(&h.env, "eng"));
    let rem = h.client.consume(&h.owner, &id(&h.env, "eng"), &100);
    assert_eq!(rem, 900);
}

#[test]
fn archived_budget_rejects_consume() {
    let h = setup();
    h.client.allocate(
        &h.owner,
        &id(&h.env, "eng"),
        &1_000,
        &Period::None,
        &false,
        &0,
    );
    h.client.archive(&h.owner, &id(&h.env, "eng"));
    let res = h.client.try_consume(&h.owner, &id(&h.env, "eng"), &100);
    assert_eq!(res, Err(Ok(Error::BudgetArchived)));
}

#[test]
fn daily_budget_auto_resets_after_window() {
    let h = setup();
    h.client.allocate(
        &h.owner,
        &id(&h.env, "eng"),
        &1_000,
        &Period::Daily,
        &false,
        &0,
    );
    h.client.consume(&h.owner, &id(&h.env, "eng"), &1_000);
    // Exhausted within the window.
    let res = h.client.try_consume(&h.owner, &id(&h.env, "eng"), &1);
    assert_eq!(res, Err(Ok(Error::BudgetExceeded)));
    // Advance one full day; the window rolls over and spending resets.
    h.env.ledger().set_timestamp(1_000 + 86_400);
    assert_eq!(h.client.remaining(&id(&h.env, "eng")), 1_000);
    let rem = h.client.consume(&h.owner, &id(&h.env, "eng"), &250);
    assert_eq!(rem, 750);
}

#[test]
fn rollover_carries_unspent_into_next_period() {
    let h = setup();
    // Weekly budget with rollover enabled, starting at t=1_000.
    h.client.allocate(
        &h.owner,
        &id(&h.env, "eng"),
        &1_000,
        &Period::Weekly,
        &true,
        &0,
    );
    h.client.consume(&h.owner, &id(&h.env, "eng"), &600);
    assert_eq!(h.client.remaining(&id(&h.env, "eng")), 400);
    // Advance past the weekly window; unspent (400) rolls over into the new period.
    h.env.ledger().set_timestamp(1_000 + 604_800);
    // New effective capacity = base limit (1000) + rollover credit (400) = 1400.
    assert_eq!(h.client.remaining(&id(&h.env, "eng")), 1_400);
    let b: Budget = h.client.get(&id(&h.env, "eng"));
    assert_eq!(b.rollover_credit, 400);
    assert_eq!(b.spent, 0);
    // Can now spend up to 1400.
    let rem = h.client.consume(&h.owner, &id(&h.env, "eng"), &1_400);
    assert_eq!(rem, 0);
}

#[test]
fn rollover_disabled_clears_unspent() {
    let h = setup();
    h.client.allocate(
        &h.owner,
        &id(&h.env, "eng"),
        &1_000,
        &Period::Weekly,
        &false,
        &0,
    );
    h.client.consume(&h.owner, &id(&h.env, "eng"), &600);
    h.env.ledger().set_timestamp(1_000 + 604_800);
    // Rollover disabled: unspent is cleared, capacity stays at the base limit.
    assert_eq!(h.client.remaining(&id(&h.env, "eng")), 1_000);
    let b: Budget = h.client.get(&id(&h.env, "eng"));
    assert_eq!(b.rollover_credit, 0);
}

#[test]
fn explicit_rollover_requires_owner() {
    let h = setup();
    h.client.allocate(
        &h.owner,
        &id(&h.env, "eng"),
        &1_000,
        &Period::Weekly,
        &true,
        &0,
    );
    h.client.consume(&h.owner, &id(&h.env, "eng"), &600);
    // Stranger cannot trigger rollover.
    let stranger = Address::generate(&h.env);
    let res = h.client.try_rollover(&stranger, &id(&h.env, "eng"));
    assert_eq!(res, Err(Ok(Error::Unauthorized)));
    // Owner advances ledger and triggers rollover explicitly.
    h.env.ledger().set_timestamp(1_000 + 604_800);
    h.client.rollover(&h.owner, &id(&h.env, "eng"));
    assert_eq!(h.client.remaining(&id(&h.env, "eng")), 1_400);
}

#[test]
fn expired_budget_rejects_consume() {
    let h = setup();
    // Expires at t = 10_000.
    h.client.allocate(
        &h.owner,
        &id(&h.env, "eng"),
        &1_000,
        &Period::None,
        &false,
        &10_000,
    );
    // Before expiry, spending works.
    let rem = h.client.consume(&h.owner, &id(&h.env, "eng"), &100);
    assert_eq!(rem, 900);
    // Past expiry, consumption is rejected.
    h.env.ledger().set_timestamp(20_000);
    let res = h.client.try_consume(&h.owner, &id(&h.env, "eng"), &100);
    assert_eq!(res, Err(Ok(Error::BudgetExpired)));
    assert_eq!(h.client.remaining(&id(&h.env, "eng")), 0);
}

#[test]
fn expired_budget_rejects_reset_and_set_limit() {
    let h = setup();
    h.client.allocate(
        &h.owner,
        &id(&h.env, "eng"),
        &1_000,
        &Period::None,
        &false,
        &10_000,
    );
    h.env.ledger().set_timestamp(20_000);
    let res = h.client.try_reset(&h.owner, &id(&h.env, "eng"));
    assert_eq!(res, Err(Ok(Error::BudgetExpired)));
    let res = h.client.try_set_limit(&h.owner, &id(&h.env, "eng"), &2_000);
    assert_eq!(res, Err(Ok(Error::BudgetExpired)));
}

#[test]
fn set_limit_below_spent_rejected() {
    let h = setup();
    h.client.allocate(
        &h.owner,
        &id(&h.env, "eng"),
        &1_000,
        &Period::None,
        &false,
        &0,
    );
    h.client.consume(&h.owner, &id(&h.env, "eng"), &600);
    let res = h.client.try_set_limit(&h.owner, &id(&h.env, "eng"), &500);
    assert_eq!(res, Err(Ok(Error::InvalidInput)));
    // Raising the limit works and increases remaining.
    h.client.set_limit(&h.owner, &id(&h.env, "eng"), &2_000);
    assert_eq!(h.client.remaining(&id(&h.env, "eng")), 1_400);
}

#[test]
fn transfer_allocation_moves_unspent_limit() {
    let h = setup();
    h.client.allocate(
        &h.owner,
        &id(&h.env, "eng"),
        &1_000,
        &Period::None,
        &false,
        &0,
    );
    h.client.allocate(
        &h.owner,
        &id(&h.env, "ops"),
        &500,
        &Period::None,
        &false,
        &0,
    );
    h.client
        .transfer_allocation(&h.owner, &id(&h.env, "eng"), &id(&h.env, "ops"), &300);
    assert_eq!(h.client.remaining(&id(&h.env, "eng")), 700);
    assert_eq!(h.client.remaining(&id(&h.env, "ops")), 800);
}

#[test]
fn transfer_allocation_over_available_fails() {
    let h = setup();
    h.client.allocate(
        &h.owner,
        &id(&h.env, "eng"),
        &1_000,
        &Period::None,
        &false,
        &0,
    );
    h.client.allocate(
        &h.owner,
        &id(&h.env, "ops"),
        &500,
        &Period::None,
        &false,
        &0,
    );
    h.client.consume(&h.owner, &id(&h.env, "eng"), &900);
    // Only 100 unspent remains in "eng".
    let res =
        h.client
            .try_transfer_allocation(&h.owner, &id(&h.env, "eng"), &id(&h.env, "ops"), &200);
    assert_eq!(res, Err(Ok(Error::BudgetExceeded)));
}

#[test]
fn get_missing_budget_fails_not_found() {
    let h = setup();
    let res = h.client.try_get(&id(&h.env, "nope"));
    assert_eq!(res, Err(Ok(Error::NotFound)));
}
