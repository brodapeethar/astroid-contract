//! Deterministic, protocol-wide error codes.
//!
//! Every contract returns variants of this single enum so that off-chain
//! consumers (the Astroid API, SDK and dashboard) can map a stable `u32` code
//! to a meaningful message. Numeric values are grouped by domain and MUST NOT
//! be reordered or reused once released — they are part of the public ABI.
//!
//! **Soroban limit:** the `#[contracterror]` macro supports at most 50 variants.

use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    // --- Generic / lifecycle (1-6) ---
    NotFound = 1,
    AlreadyExists = 2,
    Unauthorized = 3,
    InvalidInput = 4,
    NotInitialized = 5,
    AlreadyInitialized = 6,

    // --- Value / arithmetic (10-13) ---
    InsufficientFunds = 10,
    Overflow = 11,
    InvalidAmount = 12,
    Underflow = 13,

    // --- Policy (20-26) ---
    PolicyDenied = 20,
    EmergencyLock = 21,
    AssetNotWhitelisted = 25,
    PolicyPaused = 26,

    // --- Registry (30-31) ---
    RegistryFrozen = 30,
    ModuleDeprecated = 31,

    // --- Budget (40-44) ---
    BudgetExceeded = 40,
    BudgetArchived = 42,
    AssetNotAuthorized = 43,
    BudgetExpired = 44,

    // --- Wallet (50-55) ---
    WalletFrozen = 50,
    WalletArchived = 51,
    WalletPaused = 52,
    InvalidState = 53,
    UnauthorizedDispatch = 54,
    ReserveViolation = 55,

    // --- Multisig / approvals (61-69, 90-92) ---
    ThresholdNotMet = 61,
    AlreadySigned = 62,
    NotASigner = 63,
    InvalidThreshold = 64,
    TooManySigners = 66,
    /// A sub-call within a batch failed; the entire batch reverted atomically.
    BatchCallFailed = 67,
    /// Batch nonce is not strictly greater than the last used nonce (replay).
    InvalidNonce = 68,
    /// A signer with zero (or otherwise invalid) voting weight was supplied.
    InvalidSignerWeight = 69,
    /// Accumulated approval weight is below the configured threshold.
    InsufficientWeight = 90,
    /// A timelocked governance change was executed before its delay elapsed.
    TimelockNotExpired = 91,
    /// A caller without governance rights attempted to modify signers,
    /// weights or the threshold.
    UnauthorizedModification = 92,

    // --- Proposal (71-79) ---
    ProposalExpired = 71,
    InvalidProposalState = 72,
    ProposalNotApproved = 73,
    /// A prerequisite proposal has not executed, so the dependent proposal may
    /// not execute yet.
    PrerequisiteNotMet = 78,
    /// A declared dependency would close a cycle in the dependency graph.
    CircularDependencyDetected = 79,

    // --- Escrow (80-87) ---
    ConditionNotMet = 80,
    EscrowNotFunded = 81,
    EscrowExpired = 82,
    InvalidCondition = 83,
    TimeLockActive = 84,
    GraceActive = 87,

    // --- Treasury allowances (88-89) ---
    AllowanceExceeded = 88,
    AllowanceExpired = 89,
}
