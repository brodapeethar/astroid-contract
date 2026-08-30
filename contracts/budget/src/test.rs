#![cfg(test)]
extern crate std;

use crate::{Budget, BudgetContract, BudgetContractClient, Period};
use astroid_shared::errors::Error;
use astroid_shared::types::ResourceState;
use soroban_sdk::testutils::Events;
use soroban_sdk::testutils::{Address as _, Ledger};
use soroban_sdk::{Address, Env, IntoVal, String, Symbol, Val};

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
    assert_eq!(res, Err(Ok(Error::InvalidState)));
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

// ---------------------------------------------------------------------------
// Recurring allowance hooks
// ---------------------------------------------------------------------------

const DAY: u64 = 86_400;
const WEEK: u64 = 604_800;

/// Assert that the canonical `ContractEvent` with the given variant symbol was
/// published during the test (single-topic event = the variant name).
fn assert_event(env: &Env, variant: &str) {
    let want: Val = Symbol::new(env, variant).into_val(env);
    let found = env
        .events()
        .all()
        .iter()
        .any(|(_contract_id, topics, _data)| topics.contains(&want));
    assert!(found, "expected ContractEvent::{} to be emitted", variant);
}

/// Allocate a budget under the harness owner with the common defaults.
fn allocate(h: &Harness, budget_id: &str, limit: i128, period: Period, rollover: bool) {
    h.client.allocate(
        &h.owner,
        &id(&h.env, budget_id),
        &limit,
        &period,
        &rollover,
        &0,
    );
}

#[test]
fn several_elapsed_periods_are_all_settled_at_once() {
    let h = setup();
    allocate(&h, "eng", 1_000, Period::Weekly, true);
    h.client.consume(&h.owner, &id(&h.env, "eng"), &600);

    // Nobody touches the budget for three whole weeks.
    h.env.ledger().set_timestamp(1_000 + 3 * WEEK);

    // Week 1 leaves 400 unspent; weeks 2 and 3 went by entirely unspent and
    // contribute a full base limit each: 400 + 1_000 + 1_000 = 2_400 credit.
    assert_eq!(h.client.remaining(&id(&h.env, "eng")), 1_000 + 2_400);
    let b: Budget = h.client.get(&id(&h.env, "eng"));
    assert_eq!(b.rollover_credit, 2_400);
    assert_eq!(b.spent, 0);
    // The window is re-anchored to the period boundary, not to "now".
    assert_eq!(b.window_start, 1_000 + 3 * WEEK);
}

#[test]
fn several_elapsed_periods_without_rollover_reset_once() {
    let h = setup();
    allocate(&h, "eng", 1_000, Period::Weekly, false);
    h.client.consume(&h.owner, &id(&h.env, "eng"), &600);

    h.env.ledger().set_timestamp(1_000 + 5 * WEEK);
    // No rollover: idle periods accrue nothing, the budget simply starts over.
    assert_eq!(h.client.remaining(&id(&h.env, "eng")), 1_000);
    let b: Budget = h.client.get(&id(&h.env, "eng"));
    assert_eq!(b.rollover_credit, 0);
    assert_eq!(b.window_start, 1_000 + 5 * WEEK);
}

#[test]
fn windows_do_not_drift_when_transitions_land_mid_period() {
    let h = setup();
    allocate(&h, "eng", 1_000, Period::Daily, false);

    // A query part-way through the second day settles day one only, and anchors
    // the window to the day boundary rather than to the moment of the query.
    h.env.ledger().set_timestamp(1_000 + DAY + 100);
    assert_eq!(h.client.remaining(&id(&h.env, "eng")), 1_000);
    assert_eq!(h.client.get(&id(&h.env, "eng")).window_start, 1_000 + DAY);

    // Because the anchor did not drift, the next reset still falls due on the
    // original schedule.
    h.env.ledger().set_timestamp(1_000 + 2 * DAY);
    h.client.consume(&h.owner, &id(&h.env, "eng"), &10);
    assert_eq!(
        h.client.get(&id(&h.env, "eng")).window_start,
        1_000 + 2 * DAY
    );
}

#[test]
fn rollover_credit_is_clamped_to_its_cap() {
    let h = setup();
    allocate(&h, "eng", 1_000, Period::Weekly, true);
    // Cap the accrual at 1_500 so a long idle stretch cannot build up a
    // balance the agent could drain in a single period.
    h.client.set_recurrence(
        &h.owner,
        &id(&h.env, "eng"),
        &Period::Weekly,
        &0,
        &true,
        &1_500,
    );

    h.env.ledger().set_timestamp(1_000 + 10 * WEEK);
    // Uncapped this would be 10_000; the cap holds it at 1_500.
    assert_eq!(h.client.remaining(&id(&h.env, "eng")), 1_000 + 1_500);
    assert_eq!(h.client.get(&id(&h.env, "eng")).rollover_credit, 1_500);
}

#[test]
fn custom_period_recurs_on_its_configured_interval() {
    let h = setup();
    allocate(&h, "agent", 1_000, Period::None, false);
    // An hourly agent allowance.
    h.client.set_recurrence(
        &h.owner,
        &id(&h.env, "agent"),
        &Period::Custom,
        &3_600,
        &false,
        &0,
    );
    let b: Budget = h.client.get(&id(&h.env, "agent"));
    assert_eq!(b.period, Period::Custom);
    assert_eq!(b.period_seconds, 3_600);

    h.client.consume(&h.owner, &id(&h.env, "agent"), &1_000);
    assert_eq!(h.client.remaining(&id(&h.env, "agent")), 0);

    // Just short of the hour the allowance is still exhausted.
    h.env.ledger().set_timestamp(1_000 + 3_599);
    assert_eq!(h.client.remaining(&id(&h.env, "agent")), 0);

    // On the hour it replenishes.
    h.env.ledger().set_timestamp(1_000 + 3_600);
    assert_eq!(h.client.remaining(&id(&h.env, "agent")), 1_000);
}

#[test]
fn custom_period_requires_an_interval() {
    let h = setup();
    allocate(&h, "agent", 1_000, Period::None, false);
    let res = h.client.try_set_recurrence(
        &h.owner,
        &id(&h.env, "agent"),
        &Period::Custom,
        &0,
        &false,
        &0,
    );
    assert_eq!(res, Err(Ok(Error::InvalidInput)));

    let res = h.client.try_set_recurrence(
        &h.owner,
        &id(&h.env, "agent"),
        &Period::Daily,
        &0,
        &true,
        &-1,
    );
    assert_eq!(res, Err(Ok(Error::InvalidAmount)));
}

#[test]
fn set_recurrence_settles_the_old_policy_before_switching() {
    let h = setup();
    allocate(&h, "eng", 1_000, Period::Daily, true);
    h.client.consume(&h.owner, &id(&h.env, "eng"), &400);

    // A day has already turned over when the cadence is changed to weekly.
    h.env.ledger().set_timestamp(1_000 + DAY);
    h.client
        .set_recurrence(&h.owner, &id(&h.env, "eng"), &Period::Weekly, &0, &true, &0);

    // The reset owed under the daily policy was applied, not discarded.
    let b: Budget = h.client.get(&id(&h.env, "eng"));
    assert_eq!(b.period, Period::Weekly);
    assert_eq!(b.spent, 0);
    assert_eq!(b.rollover_credit, 600);
    // ...and the new cadence counts from the switch.
    assert_eq!(b.window_start, 1_000 + DAY);
}

#[test]
fn set_recurrence_requires_the_owner() {
    let h = setup();
    allocate(&h, "eng", 1_000, Period::Daily, false);
    let stranger = Address::generate(&h.env);
    let res = h.client.try_set_recurrence(
        &stranger,
        &id(&h.env, "eng"),
        &Period::Weekly,
        &0,
        &false,
        &0,
    );
    assert_eq!(res, Err(Ok(Error::Unauthorized)));
}

#[test]
fn disabling_rollover_drops_accrued_credit() {
    let h = setup();
    allocate(&h, "eng", 1_000, Period::Weekly, true);
    h.client.consume(&h.owner, &id(&h.env, "eng"), &200);
    h.env.ledger().set_timestamp(1_000 + WEEK);
    assert_eq!(h.client.remaining(&id(&h.env, "eng")), 1_800);

    h.client.set_recurrence(
        &h.owner,
        &id(&h.env, "eng"),
        &Period::Weekly,
        &0,
        &false,
        &0,
    );
    assert_eq!(h.client.get(&id(&h.env, "eng")).rollover_credit, 0);
    assert_eq!(h.client.remaining(&id(&h.env, "eng")), 1_000);
}

#[test]
fn consume_across_a_boundary_spends_the_replenished_allowance() {
    let h = setup();
    allocate(&h, "eng", 1_000, Period::Daily, false);
    h.client.consume(&h.owner, &id(&h.env, "eng"), &900);
    let res = h.client.try_consume(&h.owner, &id(&h.env, "eng"), &200);
    assert_eq!(res, Err(Ok(Error::BudgetExceeded)));

    // The disbursement itself evaluates the transition hook, so the very first
    // spend of the new period already sees the replenished allowance.
    h.env.ledger().set_timestamp(1_000 + DAY);
    let rem = h.client.consume(&h.owner, &id(&h.env, "eng"), &200);
    assert_eq!(rem, 800);
}

#[test]
fn rollover_and_reset_events_are_emitted() {
    let h = setup();
    allocate(&h, "eng", 1_000, Period::Weekly, true);
    h.client.consume(&h.owner, &id(&h.env, "eng"), &600);
    h.env.ledger().set_timestamp(1_000 + WEEK);
    h.client.consume(&h.owner, &id(&h.env, "eng"), &1);
    assert_event(&h.env, "BudgetUpdated");
}

// --- per-asset recurring limits ---

#[test]
fn per_asset_limit_replenishes_on_its_own_window() {
    let h = setup();
    allocate(&h, "eng", 10_000, Period::None, false);
    let token = Address::generate(&h.env);
    // 100 per hour for this token.
    h.client
        .set_budget_limit(&h.owner, &id(&h.env, "eng"), &token, &100, &3_600);

    h.client
        .check_and_record_spend(&h.owner, &id(&h.env, "eng"), &token, &80);
    assert_eq!(h.client.asset_remaining(&id(&h.env, "eng"), &token), 20);
    let res = h
        .client
        .try_check_and_record_spend(&h.owner, &id(&h.env, "eng"), &token, &30);
    assert_eq!(res, Err(Ok(Error::BudgetExceeded)));

    // The hour turns over and the per-asset allowance is whole again.
    h.env.ledger().set_timestamp(1_000 + 3_600);
    assert_eq!(h.client.asset_remaining(&id(&h.env, "eng"), &token), 100);
    h.client
        .check_and_record_spend(&h.owner, &id(&h.env, "eng"), &token, &100);
    assert_eq!(h.client.asset_remaining(&id(&h.env, "eng"), &token), 0);
    let b = h.client.get_asset_budget(&id(&h.env, "eng"), &token);
    assert_eq!(b.window_start, 1_000 + 3_600);
    assert_eq!(b.window_seconds, 3_600);
}

#[test]
fn per_asset_limit_without_a_window_never_resets() {
    let h = setup();
    allocate(&h, "eng", 10_000, Period::None, false);
    let token = Address::generate(&h.env);
    h.client
        .set_budget_limit(&h.owner, &id(&h.env, "eng"), &token, &100, &0);
    h.client
        .check_and_record_spend(&h.owner, &id(&h.env, "eng"), &token, &100);

    h.env.ledger().set_timestamp(1_000 + 10 * DAY);
    assert_eq!(h.client.asset_remaining(&id(&h.env, "eng"), &token), 0);
    let res = h
        .client
        .try_check_and_record_spend(&h.owner, &id(&h.env, "eng"), &token, &1);
    assert_eq!(res, Err(Ok(Error::BudgetExceeded)));
}

#[test]
fn per_asset_window_catches_up_across_many_periods() {
    let h = setup();
    allocate(&h, "eng", 10_000, Period::None, false);
    let token = Address::generate(&h.env);
    h.client
        .set_budget_limit(&h.owner, &id(&h.env, "eng"), &token, &100, &3_600);
    h.client
        .check_and_record_spend(&h.owner, &id(&h.env, "eng"), &token, &100);

    // Five hours later the allowance is one period's worth, not five.
    h.env.ledger().set_timestamp(1_000 + 5 * 3_600);
    assert_eq!(h.client.asset_remaining(&id(&h.env, "eng"), &token), 100);
    let b = h.client.get_asset_budget(&id(&h.env, "eng"), &token);
    assert_eq!(b.window_start, 1_000 + 5 * 3_600);
}

#[test]
fn unknown_asset_budget_is_rejected() {
    let h = setup();
    allocate(&h, "eng", 10_000, Period::None, false);
    let token = Address::generate(&h.env);
    let res = h.client.try_asset_remaining(&id(&h.env, "eng"), &token);
    assert_eq!(res, Err(Ok(Error::AssetNotAuthorized)));
}

#[test]
fn test_rollover_prevention() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let contract_id = env.register_contract(None, BudgetContract);
    let client = BudgetContractClient::new(&env, &contract_id);

    let token = Address::generate(&env);
    let b_id = soroban_sdk::String::from_str(&env, "b1");

    client.allocate(&owner, &b_id, &1000, &crate::Period::None, &false, &0);
    client.set_budget_limit(&owner, &b_id, &token, &100, &3600); // 1 hour window

    env.ledger().set_timestamp(100);
    client.check_and_record_spend(&owner, &b_id, &token, &60);

    // if they spend 50 more in same window, it should fail
    let res = client.try_check_and_record_spend(&owner, &b_id, &token, &50);
    assert_eq!(res, Err(Ok(Error::BudgetExceeded)));

    // fast forward 1 hour (3600 seconds)
    env.ledger().set_timestamp(100 + 3600 + 1);

    // Now it should succeed because window resets!
    client.check_and_record_spend(&owner, &b_id, &token, &50);
}

// --- Issue #35: Deficit carryforward tests ---

#[test]
fn deficit_carryforward_allows_overspend() {
    let h = setup();
    h.client.allocate_with_deficit(
        &h.owner,
        &id(&h.env, "eng"),
        &1_000,
        &Period::Weekly,
        &true,
        &true, // allow_deficit
        &0,
    );
    // Spend beyond the limit — deficit allowed.
    let rem = h.client.consume(&h.owner, &id(&h.env, "eng"), &1_200);
    assert_eq!(rem, -200); // negative remaining = deficit
    let b: Budget = h.client.get(&id(&h.env, "eng"));
    assert!(b.allow_deficit);
    assert_eq!(b.spent, 1_200);
}

#[test]
fn deficit_carryforward_reduces_next_period() {
    let h = setup();
    h.client.allocate_with_deficit(
        &h.owner,
        &id(&h.env, "eng"),
        &1_000,
        &Period::Weekly,
        &true,
        &true, // allow_deficit
        &0,
    );
    // Spend 1200 (200 over limit)
    h.client.consume(&h.owner, &id(&h.env, "eng"), &1_200);
    // Advance past the weekly window
    h.env.ledger().set_timestamp(1_000 + 604_800);
    // Call remaining to trigger window transition and persist the rollover state
    // Use rollover to trigger the window transition explicitly
    h.client.rollover(&h.owner, &id(&h.env, "eng"));
    let b: Budget = h.client.get(&id(&h.env, "eng"));
    assert_eq!(b.window_start, 1_000 + 604_800);
    assert_eq!(b.deficit_amount, 200);
    assert_eq!(b.spent, 0);
    // effective_capacity = limit (1000) - deficit (200) = 800
    assert_eq!(h.client.remaining(&id(&h.env, "eng")), 800);
    // Can spend up to 800 (1000 - 200 deficit)
    let rem = h.client.consume(&h.owner, &id(&h.env, "eng"), &800);
    assert_eq!(rem, 0);
    // One more unit should fail since effective capacity is exhausted
    let res = h.client.try_consume(&h.owner, &id(&h.env, "eng"), &1);
    assert_eq!(res, Err(Ok(Error::BudgetExceeded)));
}

#[test]
fn deficit_not_allowed_rejects_overspend() {
    let h = setup();
    h.client.allocate(
        &h.owner,
        &id(&h.env, "eng"),
        &1_000,
        &Period::Weekly,
        &true,
        &0,
    );
    // Spending beyond limit should fail without allow_deficit
    let res = h.client.try_consume(&h.owner, &id(&h.env, "eng"), &1_200);
    assert_eq!(res, Err(Ok(Error::BudgetExceeded)));
}

#[test]
fn deficit_without_period_rejected() {
    let h = setup();
    // Deficit carryforward requires a recurring period
    let res = h.client.try_allocate_with_deficit(
        &h.owner,
        &id(&h.env, "eng"),
        &1_000,
        &Period::None,
        &false,
        &true, // allow_deficit
        &0,
    );
    assert_eq!(res, Err(Ok(Error::InvalidInput)));
}

#[test]
fn deficit_surplus_rollover_combined() {
    let h = setup();
    h.client.allocate_with_deficit(
        &h.owner,
        &id(&h.env, "eng"),
        &1_000,
        &Period::Weekly,
        &true,
        &true, // allow_deficit
        &0,
    );
    // Spend only 600 — surplus of 400
    h.client.consume(&h.owner, &id(&h.env, "eng"), &600);
    h.env.ledger().set_timestamp(1_000 + 604_800);
    // Call remaining to trigger window transition and persist rollover state
    let rem = h.client.remaining(&id(&h.env, "eng"));
    assert_eq!(rem, 1_400);
    // After rollover: deficit=0, rollover_credit=400, spent=0
    let b: Budget = h.client.get(&id(&h.env, "eng"));
    assert_eq!(b.deficit_amount, 0);
    assert_eq!(b.rollover_credit, 400);
    assert_eq!(b.spent, 0);
}
