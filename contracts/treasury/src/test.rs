#![cfg(test)]
extern crate std;

use soroban_sdk::{
    testutils::Address as _, testutils::Events, token, vec, Address, Env, IntoVal, String, Symbol,
    Val, Vec,
};

use astroid_shared::constants::MAX_BATCH_PAYMENTS;
use astroid_shared::errors::Error;
use astroid_shared::types::Payment;

use crate::{TreasuryContract, TreasuryContractClient};

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

struct Harness<'a> {
    env: Env,
    client: TreasuryContractClient<'a>,
    admin: Address,
    asset: Address,
}

/// Register a treasury plus a test SAC token, and mint `funded` of the asset to
/// the admin so deposits move real value.
fn setup(org: &str, funded: i128) -> Harness<'static> {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);

    let id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(&env, &id);
    client.initialize(&String::from_str(&env, org), &admin);

    let token_admin = Address::generate(&env);
    let asset = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    if funded > 0 {
        token::StellarAssetClient::new(&env, &asset).mint(&admin, &funded);
    }

    Harness {
        env,
        client,
        admin,
        asset,
    }
}

fn token_balance(h: &Harness, who: &Address) -> i128 {
    token::TokenClient::new(&h.env, &h.asset).balance(who)
}

#[test]
fn full_flow_deposit_allocate_withdraw() {
    let h = setup("vault", 1_000);
    let recipient = Address::generate(&h.env);

    h.client.deposit(&h.admin, &h.asset, &1_000);
    // Internal accounting and real custody both reflect the deposit.
    assert_eq!(h.client.holding(&h.asset).total_in, 1_000);
    assert_eq!(token_balance(&h, &h.admin), 0);
    assert_eq!(token_balance(&h, &h.client.address), 1_000);

    h.client
        .allocate_budget(&h.admin, &h.asset, &String::from_str(&h.env, "maint"));

    h.client.withdraw(&h.admin, &h.asset, &recipient, &400);
    let holding = h.client.holding(&h.asset);
    assert_eq!(holding.total_in, 600);
    assert_eq!(holding.total_out, 400);
    // Real tokens left custody and reached the recipient.
    assert_eq!(token_balance(&h, &recipient), 400);
    assert_eq!(token_balance(&h, &h.client.address), 600);
}

#[test]
fn withdraw_rejected_when_not_admin() {
    let h = setup("vault", 500);
    let intruder = Address::generate(&h.env);
    h.client.deposit(&h.admin, &h.asset, &500);

    // intruder is not the admin — refused before any value moves.
    let res = h
        .client
        .try_withdraw(&intruder, &h.asset, &Address::generate(&h.env), &100);
    assert_eq!(res, Err(Ok(Error::Unauthorized)));
    assert_eq!(token_balance(&h, &h.client.address), 500);
}

#[test]
fn withdraw_overdraws() {
    let h = setup("vault", 50);
    h.client.deposit(&h.admin, &h.asset, &50);

    let res = h
        .client
        .try_withdraw(&h.admin, &h.asset, &Address::generate(&h.env), &100);
    assert_eq!(res, Err(Ok(Error::InsufficientFunds)));
    assert_eq!(token_balance(&h, &h.client.address), 50);
}

#[test]
fn frozen_treasury_rejects_withdrawals() {
    let h = setup("vault", 1_000);
    h.client.deposit(&h.admin, &h.asset, &1_000);
    h.client.freeze(&h.admin);

    let res = h
        .client
        .try_withdraw(&h.admin, &h.asset, &Address::generate(&h.env), &10);
    assert_eq!(res, Err(Ok(Error::InvalidState)));
    assert_eq!(token_balance(&h, &h.client.address), 1_000);
}

#[test]
fn deposit_into_frozen_treasury_rejected() {
    let h = setup("vault", 1_000);
    h.client.freeze(&h.admin);
    let res = h.client.try_deposit(&h.admin, &h.asset, &100);
    assert_eq!(res, Err(Ok(Error::InvalidState)));
    // No value moved on the rejected deposit.
    assert_eq!(token_balance(&h, &h.admin), 1_000);
}

#[test]
fn prepare_holds_state() {
    let h = setup("vault", 0);
    let state = h.client.get();
    assert_eq!(state.org, String::from_str(&h.env, "vault"));
}

#[test]
fn standard_events_emitted() {
    // Configuration changes publish a TreasuryConfigUpdated event. Setting a
    // (here placeholder) policy/budget address is enough to exercise the emit
    // path; we avoid a subsequent withdraw on this env because a real policy
    // gate is not wired up.
    let h = setup("vault", 0);
    h.client.set_policy(&h.admin, &h.admin);
    assert_event(&h.env, "TreasuryConfigUpdated");
    h.client.set_budget(&h.admin, &h.admin);
    assert_event(&h.env, "TreasuryConfigUpdated");

    // A successful withdraw (no policy/budget gates configured) publishes a
    // TransferExecuted event.
    let h2 = setup("vault", 1_000);
    let recipient = Address::generate(&h2.env);
    h2.client.deposit(&h2.admin, &h2.asset, &1_000);
    h2.client.withdraw(&h2.admin, &h2.asset, &recipient, &100);
    assert_event(&h2.env, "TransferExecuted");
}

/// Build one leg of a batch payout.
fn payment(recipient: &Address, amount: i128) -> Payment {
    Payment {
        recipient: recipient.clone(),
        amount,
    }
}

#[test]
fn batch_transfer_pays_every_recipient() {
    let h = setup("vault", 1_000);
    h.client.deposit(&h.admin, &h.asset, &1_000);

    let a = Address::generate(&h.env);
    let b = Address::generate(&h.env);
    let c = Address::generate(&h.env);
    let payments: Vec<Payment> = vec![&h.env, payment(&a, 100), payment(&b, 250), payment(&c, 50)];

    h.client.batch_transfer(&h.admin, &h.asset, &payments);

    assert_eq!(token_balance(&h, &a), 100);
    assert_eq!(token_balance(&h, &b), 250);
    assert_eq!(token_balance(&h, &c), 50);
    assert_eq!(token_balance(&h, &h.client.address), 600);

    // Internal accounting mirrors the aggregate payout exactly once.
    let holding = h.client.holding(&h.asset);
    assert_eq!(holding.total_in, 600);
    assert_eq!(holding.total_out, 400);

    assert_event(&h.env, "BatchTransferExecuted");
}

#[test]
fn batch_transfer_over_balance_pays_nobody() {
    let h = setup("vault", 300);
    h.client.deposit(&h.admin, &h.asset, &300);

    let a = Address::generate(&h.env);
    let b = Address::generate(&h.env);
    // Each leg fits on its own, but the cumulative total overdraws the treasury.
    let payments: Vec<Payment> = vec![&h.env, payment(&a, 200), payment(&b, 200)];

    let res = h.client.try_batch_transfer(&h.admin, &h.asset, &payments);
    assert_eq!(res, Err(Ok(Error::InsufficientFunds)));

    // Nothing partially executed: no recipient was paid and custody is intact.
    assert_eq!(token_balance(&h, &a), 0);
    assert_eq!(token_balance(&h, &b), 0);
    assert_eq!(token_balance(&h, &h.client.address), 300);
    let holding = h.client.holding(&h.asset);
    assert_eq!(holding.total_in, 300);
    assert_eq!(holding.total_out, 0);
}

#[test]
fn batch_transfer_rolls_back_when_one_leg_is_invalid() {
    let h = setup("vault", 1_000);
    h.client.deposit(&h.admin, &h.asset, &1_000);

    let a = Address::generate(&h.env);
    let b = Address::generate(&h.env);
    let c = Address::generate(&h.env);
    // The middle leg is a zero-amount payment, which invalidates the batch.
    let payments: Vec<Payment> = vec![&h.env, payment(&a, 100), payment(&b, 0), payment(&c, 100)];

    let res = h.client.try_batch_transfer(&h.admin, &h.asset, &payments);
    assert_eq!(res, Err(Ok(Error::InvalidAmount)));

    // The legs preceding the bad one are rolled back with the rest of the batch.
    assert_eq!(token_balance(&h, &a), 0);
    assert_eq!(token_balance(&h, &c), 0);
    assert_eq!(token_balance(&h, &h.client.address), 1_000);
    assert_eq!(h.client.holding(&h.asset).total_out, 0);
}

#[test]
fn batch_transfer_rejected_when_not_admin() {
    let h = setup("vault", 500);
    h.client.deposit(&h.admin, &h.asset, &500);

    let intruder = Address::generate(&h.env);
    let recipient = Address::generate(&h.env);
    let payments: Vec<Payment> = vec![&h.env, payment(&recipient, 10)];

    let res = h.client.try_batch_transfer(&intruder, &h.asset, &payments);
    assert_eq!(res, Err(Ok(Error::Unauthorized)));
    assert_eq!(token_balance(&h, &h.client.address), 500);
}

#[test]
fn batch_transfer_rejected_when_frozen() {
    let h = setup("vault", 500);
    h.client.deposit(&h.admin, &h.asset, &500);
    h.client.freeze(&h.admin);

    let recipient = Address::generate(&h.env);
    let payments: Vec<Payment> = vec![&h.env, payment(&recipient, 10)];

    let res = h.client.try_batch_transfer(&h.admin, &h.asset, &payments);
    assert_eq!(res, Err(Ok(Error::InvalidState)));
    assert_eq!(token_balance(&h, &recipient), 0);
}

#[test]
fn batch_transfer_rejects_empty_and_oversized_batches() {
    let h = setup("vault", 1_000);
    h.client.deposit(&h.admin, &h.asset, &1_000);

    let empty: Vec<Payment> = Vec::new(&h.env);
    assert_eq!(
        h.client.try_batch_transfer(&h.admin, &h.asset, &empty),
        Err(Ok(Error::InvalidInput))
    );

    let mut oversized: Vec<Payment> = Vec::new(&h.env);
    for _ in 0..(MAX_BATCH_PAYMENTS + 1) {
        let r = Address::generate(&h.env);
        oversized.push_back(payment(&r, 1));
    }
    assert_eq!(
        h.client.try_batch_transfer(&h.admin, &h.asset, &oversized),
        Err(Ok(Error::InvalidInput))
    );
    assert_eq!(token_balance(&h, &h.client.address), 1_000);
}

#[test]
fn batch_transfer_at_the_maximum_size_succeeds() {
    let h = setup("vault", 1_000);
    h.client.deposit(&h.admin, &h.asset, &1_000);

    let mut payments: Vec<Payment> = Vec::new(&h.env);
    let mut recipients = std::vec::Vec::new();
    for _ in 0..MAX_BATCH_PAYMENTS {
        let r = Address::generate(&h.env);
        payments.push_back(payment(&r, 5));
        recipients.push(r);
    }

    h.client.batch_transfer(&h.admin, &h.asset, &payments);

    for r in recipients.iter() {
        assert_eq!(token_balance(&h, r), 5);
    }
    let holding = h.client.holding(&h.asset);
    assert_eq!(holding.total_out, 5 * MAX_BATCH_PAYMENTS as i128);
    assert_eq!(holding.total_in, 1_000 - 5 * MAX_BATCH_PAYMENTS as i128);
}
