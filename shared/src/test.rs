#![cfg(test)]
//! Unit tests for the shared math, validation and constant helpers.

use crate::constants::{INSTANCE_BUMP_AMOUNT, INSTANCE_LIFETIME_THRESHOLD, MAX_SIGNERS};
use crate::errors::Error;
use crate::math::{
    checked_abs, checked_add, checked_div, checked_mul, checked_neg, checked_rem, checked_sub,
};
use crate::validation::{
    require_non_negative_amount, require_not_expired, require_positive_amount,
    require_time_reached, require_within_amount_bounds,
};
use soroban_sdk::testutils::Ledger;
use soroban_sdk::Env;

// ---------------------------------------------------------------------------
// checked_add
// ---------------------------------------------------------------------------

#[test]
fn add_happy_path() {
    assert_eq!(checked_add(0, 0), Ok(0));
    assert_eq!(checked_add(2, 3), Ok(5));
    assert_eq!(checked_add(-2, -3), Ok(-5));
    assert_eq!(checked_add(-5, 5), Ok(0));
    assert_eq!(checked_add(5, -5), Ok(0));
}

#[test]
fn add_identity() {
    assert_eq!(checked_add(42, 0), Ok(42));
    assert_eq!(checked_add(0, 42), Ok(42));
    assert_eq!(checked_add(-42, 0), Ok(-42));
}

#[test]
fn add_overflow() {
    assert_eq!(checked_add(i128::MAX, 1), Err(Error::Overflow));
    assert_eq!(checked_sub(i128::MIN, 1), Err(Error::Underflow));
    assert_eq!(checked_mul(i128::MAX, 2), Err(Error::Overflow));
    assert_eq!(checked_mul(i128::MAX, i128::MAX), Err(Error::Overflow));
    assert_eq!(checked_mul(i128::MAX, 100), Err(Error::Overflow));
}

#[test]
fn mul_underflow() {
    // i128::MIN * 2 overflows because |i128::MIN| > i128::MAX.
    assert_eq!(checked_mul(i128::MIN, 2), Err(Error::Overflow));
    // i128::MIN * -1 overflows because -i128::MIN cannot be represented.
    assert_eq!(checked_mul(i128::MIN, -1), Err(Error::Overflow));
}

#[test]
fn mul_large_values() {
    // Both large positive — still fits.
    assert_eq!(checked_mul(1_000_000, 1_000_000), Ok(1_000_000_000_000));
    // Large negative * large positive — overflow.
    assert_eq!(
        checked_mul(-1_000_000_000_000i128, 1_000_000_000_000i128),
        Err(Error::Overflow)
    );
}

// ---------------------------------------------------------------------------
// checked_div
// ---------------------------------------------------------------------------

#[test]
fn div_happy_path() {
    assert_eq!(checked_div(20, 5), Ok(4));
    assert_eq!(checked_div(-20, 5), Ok(-4));
    assert_eq!(checked_div(20, -5), Ok(-4));
    assert_eq!(checked_div(-20, -5), Ok(4));
}

#[test]
fn div_identity() {
    assert_eq!(checked_div(42, 1), Ok(42));
    assert_eq!(checked_div(-42, 1), Ok(-42));
    assert_eq!(checked_div(42, -1), Ok(-42));
}

#[test]
fn div_zero_dividend() {
    assert_eq!(checked_div(0, 5), Ok(0));
    assert_eq!(checked_div(0, -5), Ok(0));
}

#[test]
fn div_by_zero() {
    assert_eq!(checked_div(0, 0), Err(Error::InvalidInput));
    assert_eq!(checked_div(42, 0), Err(Error::InvalidInput));
    assert_eq!(checked_div(-42, 0), Err(Error::InvalidInput));
}

#[test]
fn div_truncation() {
    // Integer division truncates toward zero.
    assert_eq!(checked_div(7, 2), Ok(3));
    assert_eq!(checked_div(-7, 2), Ok(-3));
    assert_eq!(checked_div(7, -2), Ok(-3));
}

#[test]
fn div_overflow() {
    // i128::MIN / -1 cannot be represented.
    assert_eq!(checked_div(i128::MIN, -1), Err(Error::Overflow));
}

// ---------------------------------------------------------------------------
// checked_rem
// ---------------------------------------------------------------------------

#[test]
fn rem_happy_path() {
    assert_eq!(checked_rem(7, 3), Ok(1));
    assert_eq!(checked_rem(-7, 3), Ok(-1));
    assert_eq!(checked_rem(7, -3), Ok(1));
    assert_eq!(checked_rem(-7, -3), Ok(-1));
}

#[test]
fn rem_zero_dividend() {
    assert_eq!(checked_rem(0, 5), Ok(0));
    assert_eq!(checked_rem(0, -5), Ok(0));
}

#[test]
fn rem_by_zero() {
    assert_eq!(checked_rem(42, 0), Err(Error::InvalidInput));
    assert_eq!(checked_rem(0, 0), Err(Error::InvalidInput));
}

#[test]
fn rem_exact_division() {
    assert_eq!(checked_rem(10, 5), Ok(0));
    assert_eq!(checked_rem(-10, 5), Ok(0));
}

// ---------------------------------------------------------------------------
// checked_neg
// ---------------------------------------------------------------------------

#[test]
fn neg_happy_path() {
    assert_eq!(checked_neg(0), Ok(0));
    assert_eq!(checked_neg(42), Ok(-42));
    assert_eq!(checked_neg(-42), Ok(42));
}

#[test]
fn neg_overflow() {
    // -i128::MIN cannot be represented as i128.
    assert_eq!(checked_neg(i128::MIN), Err(Error::Overflow));
}

#[test]
fn neg_extremes() {
    assert_eq!(checked_neg(i128::MAX), Ok(-i128::MAX));
    assert_eq!(checked_neg(-i128::MAX), Ok(i128::MAX));
}

// ---------------------------------------------------------------------------
// checked_abs
// ---------------------------------------------------------------------------

#[test]
fn abs_happy_path() {
    assert_eq!(checked_abs(0), Ok(0));
    assert_eq!(checked_abs(42), Ok(42));
    assert_eq!(checked_abs(-42), Ok(42));
}

#[test]
fn abs_overflow() {
    // |i128::MIN| cannot be represented as i128.
    assert_eq!(checked_abs(i128::MIN), Err(Error::Overflow));
}

#[test]
fn abs_extremes() {
    assert_eq!(checked_abs(i128::MAX), Ok(i128::MAX));
    assert_eq!(checked_abs(-i128::MAX), Ok(i128::MAX));
}

#[test]
fn math_additional_edge_cases() {
    // Underflow only when the result drops below the minimum value. Per the
    // helper contracts, `checked_add`/`checked_mul` report any wrap as Overflow
    // while `checked_sub` reports wraps as Underflow.
    assert_eq!(checked_sub(0, 1), Ok(-1));
    assert_eq!(checked_sub(i128::MIN, 1), Err(Error::Underflow));
    assert_eq!(checked_add(i128::MIN, -1), Err(Error::Overflow));
    // Multiplication overflow on the extreme negative bound.
    assert_eq!(checked_mul(i128::MIN, -1), Err(Error::Overflow));
    assert_eq!(checked_mul(i128::MIN, 2), Err(Error::Overflow));
    // Division by zero is rejected before any arithmetic is attempted.
    assert_eq!(checked_div(0, 0), Err(Error::InvalidInput));
    assert_eq!(checked_div(-7, 0), Err(Error::InvalidInput));
    // The one division that overflows: MIN / -1 has no representable result.
    assert_eq!(checked_div(i128::MIN, -1), Err(Error::Overflow));
    // Zero and identity operations stay exact.
    assert_eq!(checked_add(i128::MAX, 0), Ok(i128::MAX));
    assert_eq!(checked_mul(0, i128::MAX), Ok(0));
    assert_eq!(checked_div(i128::MIN, 1), Ok(i128::MIN));
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
    assert_eq!(
        require_time_reached(&env, 2_000),
        Err(Error::TimelockNotExpired)
    );
}

#[test]
fn constants_are_sane() {
    const _: () = {
        assert!(INSTANCE_LIFETIME_THRESHOLD < INSTANCE_BUMP_AMOUNT);
    };
    const _: () = {
        assert!(MAX_SIGNERS >= 1);
    };
    const _: () = {
        assert!(INSTANCE_LIFETIME_THRESHOLD < INSTANCE_BUMP_AMOUNT);
    };
    const _: () = {
        assert!(MAX_SIGNERS >= 1);
    };
}
