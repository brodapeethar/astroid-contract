//! Standardized cross-cutting events.
//!
//! Per PRD Doc 7 the backend subscribes to a fixed set of protocol events to
//! drive analytics, notifications and audit logs. These helpers publish those
//! events with a consistent topic/data schema so that every contract emits them
//! identically. Contracts may also publish additional, contract-specific events
//! directly; these are the shared "standard" set.
//!
//! Two layers are provided:
//!
//! 1. **Typed [`ContractEvent`]** — a single `ContractEvent` enum that is the
//!    canonical, structured schema consumed by off-chain indexers. Each variant
//!    publishes under one topic equal to the variant symbol (e.g. `WalletCreated`)
//!    with a strongly-typed payload, so consumers get stable, self-describing
//!    events across every contract.
//! 2. **Tuple-topic helpers** — convenience functions publishing the legacy
//!    `(Symbol category, Symbol action)` tuple topics, retained for backwards
//!    compatibility with existing dashboards.
//!
//! The two layers are emitted together on key state transitions so neither
//! existing nor new consumers break.

use crate::types::ModuleKind;
use soroban_sdk::{symbol_short, Address, Env, String, Symbol};

/// Canonical, structured event schema emitted by every Astroid contract.
///
/// Publish with `events::publish(env, ContractEvent::Variant { .. })`. Each
/// variant becomes a single-topic event (the variant symbol) carrying a typed
/// payload, giving off-chain indexers one stable schema to track state changes
/// such as module updates, wallet/registry state changes, treasury
/// configuration, budget allocations and policy violations.
#[derive(Clone)]
pub enum ContractEvent {
    /// A module was registered or updated in the registry.
    RegistryModuleUpdated {
        org: String,
        kind: ModuleKind,
        address: Address,
    },
    /// An organization's owner changed.
    OrgOwnerChanged { org: String, new_owner: Address },
    /// The registry was frozen (`frozen = true`) or unfrozen (`frozen = false`).
    RegistryFrozen { org: String, frozen: bool },
    /// A wallet was created.
    WalletCreated { wallet_id: u64, owner: Address },
    /// A wallet changed lifecycle state (`state` is e.g. `frozen`/`paused`/...).
    WalletStateChanged { wallet_id: u64, state: Symbol },
    /// Value moved out of a contract to a recipient.
    TransferExecuted {
        from: Address,
        to: Address,
        asset: Address,
        amount: i128,
    },
    /// Value moved out of a contract to several recipients in one atomic batch.
    /// Emitted once per batch (not once per leg) to keep the log concise; the
    /// individual token transfers remain visible as SAC events.
    BatchTransferExecuted {
        from: Address,
        asset: Address,
        count: u32,
        total: i128,
    },
    /// A treasury configuration field was updated (`action` is e.g. `policy`).
    TreasuryConfigUpdated { org: String, action: Symbol },
    /// A budget was allocated, consumed or rolled over (`action` describes which).
    BudgetUpdated {
        budget_id: String,
        action: Symbol,
        amount: i128,
    },
    /// A policy rejected a transfer.
    PolicyViolation { policy_id: String, reason: Symbol },
}

/// Publish a [`ContractEvent`] using the canonical schema.
///
/// Each variant is emitted under a single topic equal to the variant symbol
/// (e.g. `WalletCreated`) carrying the variant's fields as a typed payload, so
/// off-chain indexers get one stable, self-describing schema per event.
pub fn publish(env: &Env, event: ContractEvent) {
    match event {
        ContractEvent::RegistryModuleUpdated { org, kind, address } => {
            env.events().publish(
                (Symbol::new(env, "RegistryModuleUpdated"),),
                (org, kind, address),
            );
        }
        ContractEvent::OrgOwnerChanged { org, new_owner } => {
            env.events()
                .publish((Symbol::new(env, "OrgOwnerChanged"),), (org, new_owner));
        }
        ContractEvent::RegistryFrozen { org, frozen } => {
            env.events()
                .publish((Symbol::new(env, "RegistryFrozen"),), (org, frozen));
        }
        ContractEvent::WalletCreated { wallet_id, owner } => {
            env.events()
                .publish((Symbol::new(env, "WalletCreated"),), (wallet_id, owner));
        }
        ContractEvent::WalletStateChanged { wallet_id, state } => {
            env.events().publish(
                (Symbol::new(env, "WalletStateChanged"),),
                (wallet_id, state),
            );
        }
        ContractEvent::TransferExecuted {
            from,
            to,
            asset,
            amount,
        } => {
            env.events().publish(
                (Symbol::new(env, "TransferExecuted"),),
                (from, to, asset, amount),
            );
        }
        ContractEvent::BatchTransferExecuted {
            from,
            asset,
            count,
            total,
        } => {
            env.events().publish(
                (Symbol::new(env, "BatchTransferExecuted"),),
                (from, asset, count, total),
            );
        }
        ContractEvent::TreasuryConfigUpdated { org, action } => {
            env.events()
                .publish((Symbol::new(env, "TreasuryConfigUpdated"),), (org, action));
        }
        ContractEvent::BudgetUpdated {
            budget_id,
            action,
            amount,
        } => {
            env.events().publish(
                (Symbol::new(env, "BudgetUpdated"),),
                (budget_id, action, amount),
            );
        }
        ContractEvent::PolicyViolation { policy_id, reason } => {
            env.events()
                .publish((Symbol::new(env, "PolicyViolation"),), (policy_id, reason));
        }
    }
}

/// `WalletCreated` — topic `("wallet", "created")`.
pub fn wallet_created(env: &Env, wallet_id: u64, owner: &Address) {
    let topics = (symbol_short!("wallet"), symbol_short!("created"));
    env.events().publish(topics, (wallet_id, owner.clone()));
}

/// `WalletFrozen` — topic `("wallet", "frozen")`.
pub fn wallet_frozen(env: &Env, wallet_id: u64, by: &Address) {
    let topics = (symbol_short!("wallet"), symbol_short!("frozen"));
    env.events().publish(topics, (wallet_id, by.clone()));
}

/// `TransferExecuted` — topic `("transfer", "executed")`.
pub fn transfer_executed(env: &Env, from: &Address, to: &Address, asset: &Address, amount: i128) {
    let topics = (symbol_short!("transfer"), symbol_short!("executed"));
    env.events()
        .publish(topics, (from.clone(), to.clone(), asset.clone(), amount));
}

/// `ProposalCreated` — topic `("proposal", "created")`.
pub fn proposal_created(env: &Env, proposal_id: u64, proposer: &Address) {
    let topics = (symbol_short!("proposal"), symbol_short!("created"));
    env.events()
        .publish(topics, (proposal_id, proposer.clone()));
}

/// `ProposalApproved` — topic `("proposal", "approved")`.
pub fn proposal_approved(env: &Env, proposal_id: u64, approver: &Address, approvals: u32) {
    let topics = (symbol_short!("proposal"), symbol_short!("approved"));
    env.events()
        .publish(topics, (proposal_id, approver.clone(), approvals));
}

/// `BudgetExceeded` — topic `("budget", "exceeded")`.
pub fn budget_exceeded(env: &Env, budget_id: &String, requested: i128, remaining: i128) {
    let topics = (symbol_short!("budget"), symbol_short!("exceeded"));
    env.events()
        .publish(topics, (budget_id.clone(), requested, remaining));
}

/// `PolicyViolation` — topic `("policy", "violation")`.
pub fn policy_violation(env: &Env, policy_id: &String, reason: Symbol) {
    let topics = (symbol_short!("policy"), symbol_short!("violation"));
    env.events().publish(topics, (policy_id.clone(), reason));
}

/// `TreasuryCreated` — topic `("treasury", "created")`.
pub fn treasury_created(env: &Env, org: &String, admin: &Address) {
    let topics = (symbol_short!("treasury"), symbol_short!("created"));
    env.events().publish(topics, (org.clone(), admin.clone()));
}

/// Construct a `Symbol` reason code from a static name (used as event payloads
/// for policy/budget violations) so all call sites share one construction path.
pub fn reason(env: &Env, name: &str) -> Symbol {
    Symbol::new(env, name)
}
