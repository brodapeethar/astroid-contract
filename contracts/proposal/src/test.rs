#![cfg(test)]
extern crate std;

use crate::{ProposalContract, ProposalContractClient, ProposalState};
use astroid_shared::errors::Error;
use soroban_sdk::testutils::{Address as _, Ledger};
use soroban_sdk::{Address, Env, String, Vec};

struct Harness {
    env: Env,
    client: ProposalContractClient<'static>,
    proposer: Address,
    approvers: std::vec::Vec<Address>,
}

fn setup(num_approvers: u32) -> Harness {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1_000);
    let contract_id = env.register_contract(None, ProposalContract);
    let client = ProposalContractClient::new(&env, &contract_id);
    client.initialize();

    let proposer = Address::generate(&env);
    let mut approvers = std::vec::Vec::new();
    for _ in 0..num_approvers {
        approvers.push(Address::generate(&env));
    }
    Harness {
        env,
        client,
        proposer,
        approvers,
    }
}

fn approver_vec(h: &Harness) -> Vec<Address> {
    let mut v = Vec::new(&h.env);
    for a in &h.approvers {
        v.push_back(a.clone());
    }
    v
}

fn create(h: &Harness, threshold: u32, expires_at: u64) -> u64 {
    h.client.create(
        &h.proposer,
        &String::from_str(&h.env, "acme"),
        &String::from_str(&h.env, "wallet-1"),
        &String::from_str(&h.env, "policy-1"),
        &String::from_str(&h.env, "tx-ref-1"),
        &approver_vec(h),
        &threshold,
        &expires_at,
        &0,
    )
}

#[test]
fn create_starts_pending() {
    let h = setup(3);
    let id = create(&h, 2, 5_000);
    assert_eq!(h.client.state(&id), ProposalState::Pending);
}

#[test]
fn full_lifecycle_to_closed() {
    let h = setup(3);
    let id = create(&h, 2, 5_000);
    h.client.approve(&h.approvers[0], &id);
    let approvals = h.client.approve(&h.approvers[1], &id);
    assert_eq!(approvals, 2);
    assert_eq!(h.client.state(&id), ProposalState::Approved);

    h.client.execute(&h.proposer, &id);
    assert_eq!(h.client.state(&id), ProposalState::Executed);

    h.client.close(&h.proposer, &id);
    assert_eq!(h.client.state(&id), ProposalState::Closed);
}

#[test]
fn execute_before_approved_fails() {
    let h = setup(3);
    let id = create(&h, 2, 5_000);
    h.client.approve(&h.approvers[0], &id); // only 1 of 2
    let res = h.client.try_execute(&h.proposer, &id);
    assert_eq!(res, Err(Ok(Error::ProposalNotApproved)));
}

#[test]
fn non_approver_cannot_approve() {
    let h = setup(3);
    let id = create(&h, 2, 5_000);
    let stranger = Address::generate(&h.env);
    let res = h.client.try_approve(&stranger, &id);
    assert_eq!(res, Err(Ok(Error::NotAnApprover)));
}

#[test]
fn double_approval_rejected() {
    let h = setup(3);
    let id = create(&h, 2, 5_000);
    h.client.approve(&h.approvers[0], &id);
    let res = h.client.try_approve(&h.approvers[0], &id);
    assert_eq!(res, Err(Ok(Error::AlreadySigned)));
}

#[test]
fn reject_moves_to_rejected() {
    let h = setup(3);
    let id = create(&h, 2, 5_000);
    h.client.reject(&h.approvers[0], &id);
    assert_eq!(h.client.state(&id), ProposalState::Rejected);
    // Cannot approve a rejected proposal.
    let res = h.client.try_approve(&h.approvers[1], &id);
    assert_eq!(res, Err(Ok(Error::InvalidProposalState)));
}

#[test]
fn only_proposer_can_cancel() {
    let h = setup(3);
    let id = create(&h, 2, 5_000);
    let res = h.client.try_cancel(&h.approvers[0], &id);
    assert_eq!(res, Err(Ok(Error::Unauthorized)));
    h.client.cancel(&h.proposer, &id);
    assert_eq!(h.client.state(&id), ProposalState::Cancelled);
}

#[test]
fn expired_proposal_cannot_be_approved() {
    let h = setup(3);
    let id = create(&h, 2, 5_000);
    // Advance beyond expiry.
    h.env.ledger().set_timestamp(6_000);
    let res = h.client.try_approve(&h.approvers[0], &id);
    assert_eq!(res, Err(Ok(Error::ProposalExpired)));
    // The failed approval is rolled back by the host, so the proposal is still
    // Pending on-chain. The terminal `Expired` transition is recorded only via
    // the permissionless `expire()` path (see `explicit_expire_transition`).
    assert_eq!(h.client.state(&id), ProposalState::Pending);
    h.client.expire(&id);
    assert_eq!(h.client.state(&id), ProposalState::Expired);
}

#[test]
fn explicit_expire_transition() {
    let h = setup(3);
    let id = create(&h, 2, 5_000);
    // Cannot expire before deadline.
    let early = h.client.try_expire(&id);
    assert_eq!(early, Err(Ok(Error::InvalidProposalState)));
    h.env.ledger().set_timestamp(6_000);
    h.client.expire(&id);
    assert_eq!(h.client.state(&id), ProposalState::Expired);
}

#[test]
fn create_with_bad_threshold_fails() {
    let h = setup(2);
    // threshold 3 > 2 approvers
    let res = h.client.try_create(
        &h.proposer,
        &String::from_str(&h.env, "acme"),
        &String::from_str(&h.env, "wallet-1"),
        &String::from_str(&h.env, "policy-1"),
        &String::from_str(&h.env, "tx-ref-1"),
        &approver_vec(&h),
        &3,
        &5_000,
        &0,
    );
    assert_eq!(res, Err(Ok(Error::InvalidThreshold)));
}

#[test]
fn create_with_past_expiry_fails() {
    let h = setup(2);
    let res = h.client.try_create(
        &h.proposer,
        &String::from_str(&h.env, "acme"),
        &String::from_str(&h.env, "wallet-1"),
        &String::from_str(&h.env, "policy-1"),
        &String::from_str(&h.env, "tx-ref-1"),
        &approver_vec(&h),
        &1,
        &500, // in the past (now = 1000)
        &0,
    );
    assert_eq!(res, Err(Ok(Error::InvalidInput)));
}

#[test]
fn test_cancellation_grace_window() {
    let h = setup(3);
    h.env.ledger().set_timestamp(100);
    let id = h.client.create(
        &h.proposer,
        &String::from_str(&h.env, "org"),
        &String::from_str(&h.env, "w1"),
        &String::from_str(&h.env, "p1"),
        &String::from_str(&h.env, "tx1"),
        &approver_vec(&h),
        &2,
        &0,
        &50, // 50 seconds grace period
    );

    // Fast forward 51 seconds
    h.env.ledger().set_timestamp(151);

    // Cancel should fail
    let res = h.client.try_cancel(&h.proposer, &id);
    assert_eq!(res, Err(Ok(Error::CancellationWindowClosed)));

    // Create a new one and cancel inside window
    let id2 = h.client.create(
        &h.proposer,
        &String::from_str(&h.env, "org"),
        &String::from_str(&h.env, "w1"),
        &String::from_str(&h.env, "p1"),
        &String::from_str(&h.env, "tx1"),
        &approver_vec(&h),
        &2,
        &0,
        &50,
    );

    h.env.ledger().set_timestamp(160);
    h.client.cancel(&h.proposer, &id2); // works since 160 < 151 + 50 (created at 151)

    assert_eq!(h.client.state(&id2), crate::ProposalState::Cancelled);
}
