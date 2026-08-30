//! Deterministic, protocol-wide error codes.
//!
//! Every contract returns variants of this single enum so that off-chain
//! consumers (the Astroid API, SDK and dashboard) can map a stable `u32` code
//! to a meaningful message. Numeric values are grouped by domain and MUST NOT
//! be reordered or reused once released — they are part of the public ABI.

use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    // --- Generic / lifecycle (1-9) ---
    NotFound = 1,
    AlreadyExists = 2,
    Unauthorized = 3,
    InvalidInput = 4,
    NotInitialized = 5,
    AlreadyInitialized = 6,

    // --- Value / arithmetic (10-19) ---
    InsufficientFunds = 10,
    Overflow = 11,
    Underflow = 12,
    InvalidAmount = 13,

    // --- Policy (20-29) ---
    PolicyDenied = 20,
    PolicyHashMismatch = 21,
    EmergencyLock = 22,
    PolicyRecipientRestricted = 23,
    PolicyMerchantBlocked = 24,
    PolicyCategoryRestricted = 25,
    /// The asset is not in the organization's whitelist.
    AssetNotWhitelisted = 26,

    // --- Registry (30-39) ---
    RegistryFrozen = 30,

    // --- Budget (40-49) ---
    BudgetExceeded = 40,
    BudgetFrozen = 41,
    BudgetArchived = 42,
    AssetNotAuthorized = 43,
    BudgetExpired = 44,

    // --- Wallet (50-59) ---
    WalletFrozen = 50,
    WalletArchived = 51,
    WalletPaused = 52,
    InvalidState = 53,
    UnauthorizedDispatch = 54,

    // --- Multisig / approvals (60-69) ---
    InvalidSignature = 60,
    ThresholdNotMet = 61,
    AlreadySigned = 62,
    NotASigner = 63,
    InvalidThreshold = 64,
    TimeLocked = 65,
    TooManySigners = 66,
    /// A sub-call within a batch failed; the entire batch reverted atomically.
    BatchCallFailed = 67,
    /// Batch nonce is not strictly greater than the last used nonce (replay).
    InvalidNonce = 68,
    /// A signer with zero (or otherwise invalid) voting weight was supplied.
    InvalidSignerWeight = 69,
    /// Accumulated approval weight is below the configured threshold.
    InsufficientWeight = 90,

    // --- Proposal (70-79) ---
    ProposalExpired = 70,
    InvalidProposalState = 71,
    ProposalNotApproved = 72,
    NotAnApprover = 73,
    CancellationWindowClosed = 74,
    MathOverflow = 75,
    DivisionByZero = 76,

    // --- Escrow (80-89) ---
    ConditionNotMet = 80,
    EscrowNotFunded = 81,
    EscrowExpired = 82,
    InvalidCondition = 83,
    TimeLockActive = 84,
}
