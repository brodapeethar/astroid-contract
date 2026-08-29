//! `#[contracttype]` values reused across multiple Astroid contracts.
//!
//! Contract-specific storage structs live in each contract crate; this module
//! only holds vocabulary shared by more than one contract so that the schema
//! (and thus the off-chain decoders) stay consistent.

use soroban_sdk::{contracttype, Address, String};

/// A single asset amount. `asset` is the Stellar asset contract address (the
/// SAC / token contract); `amount` is in the token's smallest unit (stroops for
/// XLM). Amounts are always non-negative on valid paths.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssetAmount {
    pub asset: Address,
    pub amount: i128,
}

/// Coarse lifecycle state shared by wallets and other stateful resources.
/// Individual contracts may narrow which transitions are legal.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResourceState {
    Active = 0,
    Frozen = 1,
    Paused = 2,
    Archived = 3,
}

/// The kind of module/contract an address represents inside the Registry.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModuleKind {
    Wallet = 0,
    Treasury = 1,
    Multisig = 2,
    Proposal = 3,
    Policy = 4,
    Budget = 5,
    Escrow = 6,
    Organization = 7,
}

/// An organization-scoped identifier. Astroid is multi-tenant, so most records
/// are keyed by the organization they belong to.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OrgRef {
    pub org: String,
}

/// A single leg of a batch payout: how much to send to whom. The asset is
/// carried once by the enclosing call rather than repeated per recipient, so a
/// batch always disburses a single asset and the payout total can be checked
/// against one holding.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Payment {
    pub recipient: Address,
    pub amount: i128,
}
