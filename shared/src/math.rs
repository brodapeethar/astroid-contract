//! Overflow-safe `i128` arithmetic.
//!
//! Financial contracts must never rely on wrapping arithmetic. Every value
//! computation routes through these helpers, which return a contract [`Error`]
//! instead of panicking so callers can fail safely and deterministically.

use crate::errors::Error;

pub trait SafeAdd {
    fn safe_add(self, other: Self) -> Result<Self, Error>
    where
        Self: Sized;
}

pub trait SafeSub {
    fn safe_sub(self, other: Self) -> Result<Self, Error>
    where
        Self: Sized;
}

pub trait SafeMul {
    fn safe_mul(self, other: Self) -> Result<Self, Error>
    where
        Self: Sized;
}

pub trait SafeDiv {
    fn safe_div(self, other: Self) -> Result<Self, Error>
    where
        Self: Sized;
}

impl SafeAdd for i128 {
    fn safe_add(self, other: i128) -> Result<i128, Error> {
        self.checked_add(other).ok_or(Error::MathOverflow)
    }
}

impl SafeSub for i128 {
    fn safe_sub(self, other: i128) -> Result<i128, Error> {
        self.checked_sub(other).ok_or(Error::Underflow)
    }
}

impl SafeMul for i128 {
    fn safe_mul(self, other: i128) -> Result<i128, Error> {
        self.checked_mul(other).ok_or(Error::MathOverflow)
    }
}

impl SafeDiv for i128 {
    fn safe_div(self, other: i128) -> Result<i128, Error> {
        if other == 0 {
            return Err(Error::DivisionByZero);
        }
        self.checked_div(other).ok_or(Error::MathOverflow)
    }
}

// Keep the old functions for backwards compatibility in other contracts,
// but delegate to the traits
pub fn checked_add(a: i128, b: i128) -> Result<i128, Error> {
    a.safe_add(b).map_err(|_| Error::Overflow) // mapping back for old code
}

pub fn checked_sub(a: i128, b: i128) -> Result<i128, Error> {
    a.safe_sub(b)
}

pub fn checked_mul(a: i128, b: i128) -> Result<i128, Error> {
    a.safe_mul(b).map_err(|_| Error::Overflow)
}

pub fn checked_div(a: i128, b: i128) -> Result<i128, Error> {
    a.safe_div(b).map_err(|e| {
        if e == Error::DivisionByZero {
            Error::InvalidInput
        } else {
            Error::Overflow
        }
    })
}
