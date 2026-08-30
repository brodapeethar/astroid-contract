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

/// Shortest timelock delay (in seconds) that may be configured for multisig
/// governance changes. Signer-set, weight and threshold modifications always
/// sit in a pending state for at least this long so the organization has a
/// window to review and veto them.
pub const MIN_TIMELOCK_DELAY: u64 = SECONDS_PER_DAY;
/// Longest timelock delay (in seconds) that may be configured, so a hostile
/// signer cannot park governance behind an effectively infinite delay.
pub const MAX_TIMELOCK_DELAY: u64 = 30 * SECONDS_PER_DAY;
/// How long a matured governance change stays executable before it expires.
/// Stale proposals must be re-proposed rather than lying dormant indefinitely.
pub const GOVERNANCE_GRACE_PERIOD: u64 = SECONDS_PER_WEEK;

/// Upper bound on how many eligible approvers a proposal may declare.
pub const MAX_APPROVERS: u32 = 32;

/// Upper bound on how many prerequisite proposals one proposal may depend on.
/// Every prerequisite is read once when the dependent proposal executes, so
/// this caps the storage reads a single execution can incur.
pub const MAX_DEPENDENCIES: u32 = 8;

/// Upper bound on how many discrete calls a single batch may contain (gas safety).
pub const MAX_BATCH_CALLS: u32 = 16;

/// Upper bound on how many recipients a single batch payment may pay out to.
/// Batches are executed atomically, so this caps the worst-case cost of one
/// invocation (and therefore the cost of the revert when a leg fails).
pub const MAX_BATCH_PAYMENTS: u32 = 32;

/// Upper bound on how many distinct assets a single escrow agreement may hold.
pub const MAX_ESCROW_ASSETS: u32 = 10;

/// Maximum duration (seconds) allowed for a single temporary emergency pause.
/// Caps `pause(duration)` so an authorized admin cannot lock policy evaluation
/// indefinitely; indefinite pauses must go through `unpause` explicitly.
pub const MAX_PAUSE_DURATION: u64 = SECONDS_PER_MONTH;
