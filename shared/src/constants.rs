//! Protocol-wide constants: time units, storage TTL bumps and safety limits.

/// Seconds in a day — used for daily policy/budget windows.
pub const SECONDS_PER_DAY: u64 = 86_400;
/// Seconds in a week.
pub const SECONDS_PER_WEEK: u64 = 604_800;
/// Seconds in a (30-day) month — used for monthly policy/budget windows.
pub const SECONDS_PER_MONTH: u64 = 2_592_000;

/// Approximate number of ledgers closed per day on Stellar (~5s per ledger).
pub const DAY_IN_LEDGERS: u32 = 17_280;

/// Instance storage TTL bump (7 days) and the threshold at which we bump.
pub const INSTANCE_BUMP_AMOUNT: u32 = 7 * DAY_IN_LEDGERS;
pub const INSTANCE_LIFETIME_THRESHOLD: u32 = INSTANCE_BUMP_AMOUNT - DAY_IN_LEDGERS;

/// Persistent storage TTL bump (30 days) and the threshold at which we bump.
pub const PERSISTENT_BUMP_AMOUNT: u32 = 30 * DAY_IN_LEDGERS;
pub const PERSISTENT_LIFETIME_THRESHOLD: u32 = PERSISTENT_BUMP_AMOUNT - DAY_IN_LEDGERS;

/// Upper bound on the number of signers a multisig may hold (gas safety).
pub const MAX_SIGNERS: u32 = 20;
/// Minimum allowed multisig threshold.
pub const MIN_THRESHOLD: u32 = 1;

/// Upper bound on how many eligible approvers a proposal may declare.
pub const MAX_APPROVERS: u32 = 32;

/// Upper bound on how many discrete calls a single batch may contain (gas safety).
pub const MAX_BATCH_CALLS: u32 = 16;

/// Upper bound on how many recipients a single batch payment may pay out to.
/// Batches are executed atomically, so this caps the worst-case cost of one
/// invocation (and therefore the cost of the revert when a leg fails).
pub const MAX_BATCH_PAYMENTS: u32 = 32;
