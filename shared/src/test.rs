#![cfg(test)]
//! Unit tests for the shared math, validation and constant helpers.

use crate::constants::{INSTANCE_BUMP_AMOUNT, INSTANCE_LIFETIME_THRESHOLD, MAX_SIGNERS};
use crate::errors::Error;
use crate::math::{checked_add, checked_div, checked_mul, checked_sub};
use crate::validation::{
    require_non_negative_amount, require_not_expired, require_positive_amount,
    require_time_reached, require_within_amount_bounds,
};
use soroban_sdk::testutils::Ledger;
use soroban_sdk::Env;

#[test]
fn math_happy_paths() {
    assert_eq!(checked_add(2, 3), Ok(5));
    assert_eq!(checked_sub(5, 3), Ok(2));
    assert_eq!(checked_mul(4, 5), Ok(20));
    assert_eq!(checked_div(20, 5), Ok(4));
}

#[test]
fn math_overflow_underflow() {
    assert_eq!(checked_add(i128::MAX, 1), Err(Error::Overflow));
    assert_eq!(checked_sub(i128::MIN, 1), Err(Error::Underflow));
    assert_eq!(checked_mul(i128::MAX, 2), Err(Error::Overflow));
    assert_eq!(checked_div(1, 0), Err(Error::InvalidInput));
}

#[test]
fn amount_validation() {
    assert_eq!(require_positive_amount(1), Ok(()));
    assert_eq!(require_positive_amount(0), Err(Error::InvalidAmount));
    assert_eq!(require_positive_amount(-1), Err(Error::InvalidAmount));
    assert_eq!(require_non_negative_amount(0), Ok(()));
    assert_eq!(require_non_negative_amount(-5), Err(Error::InvalidAmount));
}

#[test]
fn amount_bounds() {
    // Within [10, 100].
    assert_eq!(require_within_amount_bounds(50, 10, 100), Ok(()));
    // Below min.
    assert_eq!(
        require_within_amount_bounds(5, 10, 100),
        Err(Error::PolicyDenied)
    );
    // Above max.
    assert_eq!(
        require_within_amount_bounds(150, 10, 100),
        Err(Error::PolicyDenied)
    );
    // max == 0 means unbounded above.
    assert_eq!(require_within_amount_bounds(10_000, 10, 0), Ok(()));
}

#[test]
fn time_validation() {
    let env = Env::default();
    env.ledger().set_timestamp(1_000);

    // Expiry in the future is fine; in the past/now is expired.
    assert_eq!(require_not_expired(&env, 2_000), Ok(()));
    assert_eq!(
        require_not_expired(&env, 1_000),
        Err(Error::ProposalExpired)
    );
    assert_eq!(require_not_expired(&env, 500), Err(Error::ProposalExpired));

    // Time lock: reached only once timestamp >= unlock_at.
    assert_eq!(require_time_reached(&env, 500), Ok(()));
    assert_eq!(require_time_reached(&env, 1_000), Ok(()));
    assert_eq!(require_time_reached(&env, 2_000), Err(Error::TimeLocked));
}

#[test]
fn constants_are_sane() {
    const _: () = {
        assert!(INSTANCE_LIFETIME_THRESHOLD < INSTANCE_BUMP_AMOUNT);
    };
    const _: () = {
        assert!(MAX_SIGNERS >= 1);
    };
}
