#![no_std]
#![allow(clippy::too_many_arguments)]
//! # Astroid MultiSig Contract
//!
//! Prevents unilateral spending by requiring an approval **weight** threshold
//! before an action executes (PRD Doc 7 §MultiSig). Each signer is assigned a
//! positive voting weight and the contract tracks the accumulated weight of
//! approvals against a configurable `threshold` (expressed in weight units,
//! not raw signer count). This lets organizations give different administrative
//! keys or partner entities proportionally larger influence.
//!
//! The contract owns a dynamic weighted signer set and a threshold, and manages
//! internal proposals through an approve → execute flow with an optional
//! per-proposal time lock and a global emergency lock.
//!
//! [`MultiSigContract::execute_batch`] additionally supports bundling multiple
//! discrete contract calls into one transaction: each contributing signer's
//! signature is verified (by the Soroban host) over the exact batch payload
//! `(nonce, calls)`, the aggregate signature weight must meet the threshold, and
//! the nonce makes batches replay-proof. Execution is atomic — if any sub-call
//! fails the whole batch reverts with [`Error::BatchCallFailed`] or the callee's
//! error.
//!
//! Events: `SignerAdded`, `SignerRemoved`, `ThresholdChanged`,
//! `ProposalApproved`, `ProposalExecuted`, `BatchExecuted`, `EmergencyLock`.
//!
//! Events: `SignerAdded`, `SignerRemoved`, `SignerWeightUpdated`,
//! `ThresholdChanged`, `ProposalApproved`, `ProposalExecuted`,
//! `EmergencyLock`.
//!
//! Execution below the weight threshold is rejected with
//! [`Error::InsufficientWeight`].

use astroid_shared::constants::{
    INSTANCE_BUMP_AMOUNT, INSTANCE_LIFETIME_THRESHOLD, MAX_BATCH_CALLS, MAX_SIGNERS, MIN_THRESHOLD,
    PERSISTENT_BUMP_AMOUNT, PERSISTENT_LIFETIME_THRESHOLD,
};
use astroid_shared::errors::Error;
use astroid_shared::math::checked_add;
use astroid_shared::validation::require_time_reached;
use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, vec, Address, Bytes, Env, IntoVal, Symbol,
    Val, Vec,
};

#[contracttype]
#[derive(Clone)]
enum DataKey {
    /// Config: current weighted signer set (instance).
    Signers,
    /// Config: current approval weight threshold (instance).
    Threshold,
    /// State: global emergency lock flag (instance).
    EmergencyLock,
    /// State: monotonic proposal id counter (instance).
    ProposalCount,
    /// State: proposal record by id (persistent).
    Proposal(u64),
    /// Relationship: whether a signer approved a proposal (persistent).
    Approval(u64, Address),
    /// State: last used batch nonce (instance); batches must use a greater one.
    LastBatchNonce,
}

/// A registered signer and its positive voting weight.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SignerWeight {
    pub address: Address,
    pub weight: u32,
}

/// Internal multisig proposal. `action`/`payload` describe the intended change
/// or call; the multisig only records weighted approvals and marks it executed
/// once the accumulated weight meets the threshold. Actual value movement is
/// delegated to the calling context (e.g. the Treasury) which checks
/// `is_executed`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MsProposal {
    pub proposer: Address,
    /// A short action tag, e.g. `payment`, `config`.
    pub action: Symbol,
    /// Opaque payload (e.g. serialized transfer intent / hash).
    pub payload: Bytes,
    /// Accumulated approval weight (sum of approver weights).
    pub approval_weight: u32,
    pub executed: bool,
    /// Earliest timestamp at which execution is allowed (time lock; 0 = none).
    pub unlock_at: u64,
}

/// A single discrete contract call inside a batch. `args` are raw Soroban
/// values, so any contract function can be targeted.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BatchCall {
    /// Contract to invoke.
    pub contract: Address,
    /// Function to invoke on the target contract.
    pub func: Symbol,
    /// Arguments passed to the target function.
    pub args: Vec<Val>,
}

#[contract]
pub struct MultiSigContract;

#[contractimpl]
impl MultiSigContract {
    /// Initialize with an initial weighted signer set and a weight threshold.
    /// `threshold` must be within `[MIN_THRESHOLD, total_weight]` and the signer
    /// set within `MAX_SIGNERS`, with all weights positive and addresses unique.
    pub fn initialize(env: Env, signers: Vec<SignerWeight>, threshold: u32) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Threshold) {
            return Err(Error::AlreadyInitialized);
        }
        let n = signers.len();
        if n == 0 || n > MAX_SIGNERS {
            return Err(Error::InvalidInput);
        }
        for s in signers.iter() {
            if s.weight == 0 {
                return Err(Error::InvalidSignerWeight);
            }
        }
        let total = Self::total_weight(&signers)?;
        Self::validate_threshold(threshold, total)?;
        Self::assert_unique(&signers)?;

        env.storage().instance().set(&DataKey::Signers, &signers);
        env.storage()
            .instance()
            .set(&DataKey::Threshold, &threshold);
        env.storage()
            .instance()
            .set(&DataKey::EmergencyLock, &false);
        env.storage().instance().set(&DataKey::ProposalCount, &0u64);
        env.storage()
            .instance()
            .set(&DataKey::LastBatchNonce, &0u64);
        Self::bump_instance(&env);
        Ok(())
    }

    /// Add a signer with a positive weight. Signer-gated. Rejects duplicates and
    /// over-capacity sets, and weights below 1.
    pub fn add_signer(
        env: Env,
        caller: Address,
        signer: Address,
        weight: u32,
    ) -> Result<(), Error> {
        Self::require_signer(&env, &caller)?;
        if weight == 0 {
            return Err(Error::InvalidSignerWeight);
        }
        let mut signers = Self::signers(&env)?;
        if signers.iter().any(|s| s.address == signer) {
            return Err(Error::AlreadyExists);
        }
        if signers.len() >= MAX_SIGNERS {
            return Err(Error::TooManySigners);
        }
        signers.push_back(SignerWeight {
            address: signer.clone(),
            weight,
        });
        env.storage().instance().set(&DataKey::Signers, &signers);
        Self::bump_instance(&env);
        env.events().publish(
            (symbol_short!("signer"), symbol_short!("added")),
            (signer, weight),
        );
        Ok(())
    }

    /// Remove a signer. Signer-gated. Refuses to drop the remaining total weight
    /// below the threshold, so the multisig can never become unusable.
    pub fn remove_signer(env: Env, caller: Address, signer: Address) -> Result<(), Error> {
        Self::require_signer(&env, &caller)?;
        let mut signers = Self::signers(&env)?;
        let idx: u32 = signers
            .iter()
            .position(|s| s.address == signer)
            .ok_or(Error::NotASigner)? as u32;
        let removed_weight = signers.get(idx).unwrap().weight;
        let remaining = checked_add(
            Self::total_weight(&signers)? as i128 - removed_weight as i128,
            0,
        )? as u32;
        let threshold = Self::threshold(&env)?;
        if remaining < threshold {
            return Err(Error::InvalidThreshold);
        }
        signers.remove(idx);
        env.storage().instance().set(&DataKey::Signers, &signers);
        Self::bump_instance(&env);
        env.events()
            .publish((symbol_short!("signer"), symbol_short!("removed")), signer);
        Ok(())
    }

    /// Update the voting weight of an existing signer. Signer-gated. The new
    /// weight must keep the total at or above the configured threshold; weights
    /// of 0 are rejected.
    pub fn set_signer_weight(
        env: Env,
        caller: Address,
        signer: Address,
        weight: u32,
    ) -> Result<(), Error> {
        Self::require_signer(&env, &caller)?;
        if weight == 0 {
            return Err(Error::InvalidSignerWeight);
        }
        let mut signers = Self::signers(&env)?;
        let idx: u32 = signers
            .iter()
            .position(|s| s.address == signer)
            .ok_or(Error::NotASigner)? as u32;
        let old_weight = signers.get(idx).unwrap().weight;
        let total = Self::total_weight(&signers)?;
        let new_total = checked_add(total as i128 - old_weight as i128 + weight as i128, 0)? as u32;
        let threshold = Self::threshold(&env)?;
        if new_total < threshold {
            return Err(Error::InvalidThreshold);
        }
        let mut updated = signers.get(idx).unwrap();
        updated.weight = weight;
        signers.set(idx, updated);
        env.storage().instance().set(&DataKey::Signers, &signers);
        Self::bump_instance(&env);
        env.events().publish(
            (symbol_short!("signer"), symbol_short!("weight")),
            (signer, weight),
        );
        Ok(())
    }

    /// Update the approval weight threshold. Signer-gated. Must stay within
    /// `[MIN_THRESHOLD, total_weight]`.
    pub fn set_threshold(env: Env, caller: Address, threshold: u32) -> Result<(), Error> {
        Self::require_signer(&env, &caller)?;
        let signers = Self::signers(&env)?;
        Self::validate_threshold(threshold, Self::total_weight(&signers)?)?;
        env.storage()
            .instance()
            .set(&DataKey::Threshold, &threshold);
        Self::bump_instance(&env);
        env.events().publish(
            (symbol_short!("threshold"), symbol_short!("changed")),
            threshold,
        );
        Ok(())
    }

    /// Toggle the global emergency lock (signer-gated). While locked, proposals
    /// cannot be created, approved or executed.
    pub fn set_emergency_lock(env: Env, caller: Address, locked: bool) -> Result<(), Error> {
        Self::require_signer(&env, &caller)?;
        env.storage()
            .instance()
            .set(&DataKey::EmergencyLock, &locked);
        Self::bump_instance(&env);
        env.events()
            .publish((symbol_short!("emergency"), symbol_short!("lock")), locked);
        Ok(())
    }

    /// Create a proposal. Only a signer may propose. `unlock_at` sets an optional
    /// time lock (0 = immediately executable once threshold met). The proposer's
    /// weight is counted automatically.
    pub fn propose(
        env: Env,
        proposer: Address,
        action: Symbol,
        payload: Bytes,
        unlock_at: u64,
    ) -> Result<u64, Error> {
        Self::require_not_locked(&env)?;
        Self::require_signer(&env, &proposer)?;

        let mut count: u64 = env
            .storage()
            .instance()
            .get(&DataKey::ProposalCount)
            .ok_or(Error::NotInitialized)?;
        count = checked_add(count as i128, 1)? as u64;
        let id = count;

        let proposer_weight = Self::weight_of(&env, &proposer)?;
        let proposal = MsProposal {
            proposer: proposer.clone(),
            action,
            payload,
            approval_weight: proposer_weight,
            executed: false,
            unlock_at,
        };
        env.storage()
            .persistent()
            .set(&DataKey::Proposal(id), &proposal);
        env.storage()
            .persistent()
            .set(&DataKey::Approval(id, proposer.clone()), &true);
        Self::bump_proposal(&env, id);
        env.storage()
            .instance()
            .set(&DataKey::ProposalCount, &count);
        Self::bump_instance(&env);

        env.events().publish(
            (symbol_short!("proposal"), symbol_short!("created")),
            (id, proposer),
        );
        Ok(id)
    }

    /// Approve a proposal. Only signers may approve, once each. Their weight is
    /// added to the accumulated total. Emits `ProposalApproved` with the running
    /// weight.
    pub fn approve(env: Env, caller: Address, proposal_id: u64) -> Result<u32, Error> {
        Self::require_not_locked(&env)?;
        Self::require_signer(&env, &caller)?;
        let mut proposal = Self::load_proposal(&env, proposal_id)?;
        if proposal.executed {
            return Err(Error::InvalidProposalState);
        }
        let akey = DataKey::Approval(proposal_id, caller.clone());
        if env.storage().persistent().get(&akey).unwrap_or(false) {
            return Err(Error::AlreadySigned);
        }
        let weight = Self::weight_of(&env, &caller)?;
        env.storage().persistent().set(&akey, &true);
        proposal.approval_weight =
            checked_add(proposal.approval_weight as i128, weight as i128)? as u32;
        env.storage()
            .persistent()
            .set(&DataKey::Proposal(proposal_id), &proposal);
        Self::bump_proposal(&env, proposal_id);
        env.events().publish(
            (symbol_short!("proposal"), symbol_short!("approved")),
            (proposal_id, caller, proposal.approval_weight),
        );
        Ok(proposal.approval_weight)
    }

    /// Execute a proposal once the accumulated approval weight meets the
    /// threshold and any time lock has elapsed. Marks it executed and emits
    /// `ProposalExecuted`. Rejects with [`Error::InsufficientWeight`] when the
    /// accumulated weight is below the threshold.
    pub fn execute(env: Env, caller: Address, proposal_id: u64) -> Result<(), Error> {
        Self::require_not_locked(&env)?;
        Self::require_signer(&env, &caller)?;
        let mut proposal = Self::load_proposal(&env, proposal_id)?;
        if proposal.executed {
            return Err(Error::InvalidProposalState);
        }
        let threshold = Self::threshold(&env)?;
        if proposal.approval_weight < threshold {
            return Err(Error::InsufficientWeight);
        }
        if proposal.unlock_at != 0 {
            require_time_reached(&env, proposal.unlock_at)?;
        }
        proposal.executed = true;
        env.storage()
            .persistent()
            .set(&DataKey::Proposal(proposal_id), &proposal);
        Self::bump_proposal(&env, proposal_id);
        env.events().publish(
            (symbol_short!("proposal"), symbol_short!("executed")),
            proposal_id,
        );
        Ok(())
    }

    /// Execute a batch of discrete contract calls under a single threshold
    /// verification. Bundling several calls into one transaction reduces fees
    /// and makes multi-step administrative operations atomic.
    ///
    /// - `caller` must be a current signer. Its signature is verified against
    ///   the batch payload itself and counts toward the threshold (mirroring
    ///   how a proposer auto-approves in the proposal flow), so every
    ///   contributing signer signs the exact same payload.
    /// - `nonce` must be strictly greater than the last used batch nonce, which
    ///   makes batches replay-proof; the nonce is part of the signed payload.
    /// - `calls` must be non-empty and within [`MAX_BATCH_CALLS`].
    /// - `approvers` lists any additional signers backing the batch. Every
    ///   listed approver must be a current signer and must authorize the exact
    ///   payload `(nonce, calls)`; the host cryptographically verifies each
    ///   signature and enforces replay prevention via
    ///   [`Address::require_auth_for_args`]. Duplicate entries (including the
    ///   caller) only count once. Each signer carries weight 1, so the number
    ///   of distinct signers — caller plus approvers — must meet the threshold.
    ///
    /// Execution is atomic: each call runs inside a Soroban error-handling
    /// boundary ([`Env::try_invoke_contract`]); if any sub-call fails the whole
    /// batch reverts (including the nonce), so no partial state is committed.
    /// A failing sub-call surfaces its own contract error when known, otherwise
    /// [`Error::BatchCallFailed`].
    pub fn execute_batch(
        env: Env,
        caller: Address,
        nonce: u64,
        calls: Vec<BatchCall>,
        approvers: Vec<Address>,
    ) -> Result<(), Error> {
        Self::require_not_locked(&env)?;
        let signers = Self::signers(&env)?;
        let threshold = Self::threshold(&env)?;
        if !signers.iter().any(|s| s.address == caller) {
            return Err(Error::NotASigner);
        }

        if calls.is_empty() || calls.len() > MAX_BATCH_CALLS {
            return Err(Error::InvalidInput);
        }
        // A list longer than the maximum signer set can only hold duplicates or
        // non-signers; reject it up front (gas safety).
        if approvers.len() > MAX_SIGNERS {
            return Err(Error::InvalidInput);
        }

        // Replay protection: batch nonces must be strictly increasing.
        let last_nonce: u64 = env
            .storage()
            .instance()
            .get(&DataKey::LastBatchNonce)
            .unwrap_or(0);
        if nonce <= last_nonce {
            return Err(Error::InvalidNonce);
        }

        // Aggregate signature verification over the entire batch payload: the
        // caller plus every distinct approver must be a signer and must have
        // authorized `(nonce, calls)`. Each signer carries weight 1.
        let payload = Self::batch_payload(&env, nonce, &calls);
        let mut weight: u32 = 1; // the caller's signature counts
        let mut seen = Vec::new(&env);
        seen.push_back(caller.clone());
        caller.require_auth_for_args(payload.clone());
        for approver in approvers.iter() {
            if !signers.iter().any(|s| s.address == approver) {
                return Err(Error::NotASigner);
            }
            if seen.contains(&approver) {
                continue;
            }
            seen.push_back(approver.clone());
            approver.require_auth_for_args(payload.clone());
            weight = checked_add(weight as i128, 1)? as u32;
        }
        if weight < threshold {
            return Err(Error::ThresholdNotMet);
        }

        env.storage()
            .instance()
            .set(&DataKey::LastBatchNonce, &nonce);

        // Execute every call atomically; any failure reverts the whole batch.
        for call in calls.iter() {
            Self::execute_call(&env, &call)?;
        }

        Self::bump_instance(&env);
        env.events().publish(
            (symbol_short!("batch"), symbol_short!("executed")),
            (nonce, caller, calls.len()),
        );
        Ok(())
    }

    // --- views ---

    pub fn get_proposal(env: Env, proposal_id: u64) -> Result<MsProposal, Error> {
        Self::load_proposal(&env, proposal_id)
    }

    /// Last used batch nonce. The next `execute_batch` call must pass a strictly
    /// greater nonce.
    pub fn get_last_batch_nonce(env: Env) -> u64 {
        env.storage()
            .instance()
            .get(&DataKey::LastBatchNonce)
            .unwrap_or(0)
    }

    pub fn get_signers(env: Env) -> Vec<SignerWeight> {
        Self::signers(&env).unwrap_or_else(|_| Vec::new(&env))
    }

    pub fn get_threshold(env: Env) -> Result<u32, Error> {
        Self::threshold(&env)
    }

    pub fn is_signer(env: Env, who: Address) -> bool {
        Self::signers(&env)
            .map(|s| s.iter().any(|sw| sw.address == who))
            .unwrap_or(false)
    }

    pub fn is_locked(env: Env) -> bool {
        env.storage()
            .instance()
            .get(&DataKey::EmergencyLock)
            .unwrap_or(false)
    }

    // --- internal helpers ---

    fn signers(env: &Env) -> Result<Vec<SignerWeight>, Error> {
        env.storage()
            .instance()
            .get(&DataKey::Signers)
            .ok_or(Error::NotInitialized)
    }

    fn total_weight(signers: &Vec<SignerWeight>) -> Result<u32, Error> {
        let mut total: i128 = 0;
        let len = signers.len();
        let mut i = 0;
        while i < len {
            let w = signers.get(i).unwrap().weight;
            total = checked_add(total, w as i128)?;
            i += 1;
        }
        Ok(total as u32)
    }

    fn weight_of(env: &Env, who: &Address) -> Result<u32, Error> {
        let signers = Self::signers(env)?;
        signers
            .iter()
            .find(|s| &s.address == who)
            .map(|s| s.weight)
            .ok_or(Error::NotASigner)
    }

    fn threshold(env: &Env) -> Result<u32, Error> {
        env.storage()
            .instance()
            .get(&DataKey::Threshold)
            .ok_or(Error::NotInitialized)
    }

    fn load_proposal(env: &Env, id: u64) -> Result<MsProposal, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Proposal(id))
            .ok_or(Error::NotFound)
    }

    fn require_signer(env: &Env, caller: &Address) -> Result<(), Error> {
        caller.require_auth();
        let signers = Self::signers(env)?;
        if !signers.iter().any(|s| &s.address == caller) {
            return Err(Error::NotASigner);
        }
        Ok(())
    }

    fn require_not_locked(env: &Env) -> Result<(), Error> {
        let locked: bool = env
            .storage()
            .instance()
            .get(&DataKey::EmergencyLock)
            .unwrap_or(false);
        if locked {
            return Err(Error::EmergencyLock);
        }
        Ok(())
    }

    /// Build the deterministic payload `(nonce, calls)` that every approver's
    /// signature must cover, so a signature can never be replayed against a
    /// different batch or a reused nonce.
    fn batch_payload(env: &Env, nonce: u64, calls: &Vec<BatchCall>) -> Vec<Val> {
        let nonce_val: Val = nonce.into_val(env);
        let calls_val: Val = calls.to_val();
        vec![env, nonce_val, calls_val]
    }

    /// Invoke a single batch call inside a Soroban error-handling boundary so a
    /// failure can be caught and the whole batch reverted atomically instead of
    /// aborting with an opaque trap. Returns the callee's own contract error
    /// when it is one of ours, otherwise [`Error::BatchCallFailed`].
    fn execute_call(env: &Env, call: &BatchCall) -> Result<(), Error> {
        match env.try_invoke_contract::<Val, Error>(&call.contract, &call.func, call.args.clone()) {
            Ok(Ok(_)) => Ok(()),
            // A raw `Val` always decodes, so this arm is unreachable in
            // practice; kept for exhaustiveness.
            Ok(Err(_)) => Err(Error::BatchCallFailed),
            // The callee exited with a contract error — surface it precisely.
            Err(Ok(e)) => Err(e),
            // System-level failure (panic / abort / unknown error code).
            Err(Err(_)) => Err(Error::BatchCallFailed),
        }
    }

    fn validate_threshold(threshold: u32, n: u32) -> Result<(), Error> {
        if threshold < MIN_THRESHOLD || threshold > n {
            return Err(Error::InvalidThreshold);
        }
        Ok(())
    }

    fn assert_unique(signers: &Vec<SignerWeight>) -> Result<(), Error> {
        let len = signers.len();
        let mut i = 0;
        while i < len {
            let a = signers.get(i).unwrap().address.clone();
            let mut j = i + 1;
            while j < len {
                if a == signers.get(j).unwrap().address {
                    return Err(Error::InvalidInput);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }

    fn bump_proposal(env: &Env, id: u64) {
        env.storage().persistent().extend_ttl(
            &DataKey::Proposal(id),
            PERSISTENT_LIFETIME_THRESHOLD,
            PERSISTENT_BUMP_AMOUNT,
        );
    }

    fn bump_instance(env: &Env) {
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }
}

#[cfg(test)]
mod test;
