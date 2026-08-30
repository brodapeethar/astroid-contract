import re
with open("contracts/multisig/src/lib.rs", "r") as f:
    text = f.read()

if "soroban_sdk::{Map, " not in text:
    text = text.replace("use soroban_sdk::{", "use soroban_sdk::{Map, ")

# DataKey change
text = text.replace("Signers,", "SignerWeights,")

# Initialize
init_old = """    pub fn initialize(env: Env, signers: Vec<Address>, threshold: u32) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Threshold) {
            return Err(Error::AlreadyInitialized);
        }
        let n = signers.len();
        if n == 0 || n > MAX_SIGNERS {
            return Err(Error::InvalidInput);
        }
        Self::validate_threshold(threshold, n)?;
        Self::assert_unique(&signers)?;

        env.storage().instance().set(&DataKey::Signers, &signers);"""
init_new = """    pub fn initialize(env: Env, signers: Vec<Address>, threshold: u32) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Threshold) {
            return Err(Error::AlreadyInitialized);
        }
        let n = signers.len();
        if n == 0 || n > MAX_SIGNERS {
            return Err(Error::InvalidInput);
        }
        Self::assert_unique(&signers)?;
        
        let mut weights: Map<Address, u32> = Map::new(&env);
        for s in signers.iter() {
            weights.set(s, 1);
        }
        
        if threshold < MIN_THRESHOLD || threshold > signers.len() {
            return Err(Error::InvalidThreshold);
        }

        env.storage().instance().set(&DataKey::SignerWeights, &weights);"""
text = text.replace(init_old, init_new)

# add_signer
add_old = """    pub fn add_signer(env: Env, caller: Address, signer: Address) -> Result<(), Error> {
        Self::require_signer(&env, &caller)?;
        let mut signers = Self::signers(&env)?;
        if signers.contains(&signer) {
            return Err(Error::AlreadyExists);
        }
        if signers.len() >= MAX_SIGNERS {
            return Err(Error::TooManySigners);
        }
        signers.push_back(signer.clone());
        env.storage().instance().set(&DataKey::Signers, &signers);"""
add_new = """    pub fn add_signer(env: Env, caller: Address, signer: Address, weight: u32) -> Result<(), Error> {
        Self::require_signer(&env, &caller)?;
        if weight == 0 { return Err(Error::InvalidInput); }
        let mut weights: Map<Address, u32> = env.storage().instance().get(&DataKey::SignerWeights).unwrap_or(Map::new(&env));
        if weights.contains_key(signer.clone()) {
            return Err(Error::AlreadyExists);
        }
        if weights.len() >= MAX_SIGNERS {
            return Err(Error::TooManySigners);
        }
        weights.set(signer.clone(), weight);
        env.storage().instance().set(&DataKey::SignerWeights, &weights);"""
text = text.replace(add_old, add_new)

# remove_signer
rem_old = """    pub fn remove_signer(env: Env, caller: Address, signer: Address) -> Result<(), Error> {
        Self::require_signer(&env, &caller)?;
        let mut signers = Self::signers(&env)?;
        let threshold = Self::threshold(&env)?;
        let idx = signers
            .iter()
            .position(|s| s == signer)
            .ok_or(Error::NotFound)?;
        let new_len = signers.len() - 1;
        if new_len < threshold {
            return Err(Error::InvalidThreshold);
        }
        signers.remove(idx);
        env.storage().instance().set(&DataKey::Signers, &signers);"""
rem_new = """    pub fn remove_signer(env: Env, caller: Address, signer: Address) -> Result<(), Error> {
        Self::require_signer(&env, &caller)?;
        let mut weights: Map<Address, u32> = env.storage().instance().get(&DataKey::SignerWeights).unwrap_or(Map::new(&env));
        if !weights.contains_key(signer.clone()) {
            return Err(Error::NotFound);
        }
        weights.remove(signer.clone());
        let threshold = Self::threshold(&env)?;
        
        let mut total = 0;
        for (_, w) in weights.iter() {
            total += w;
        }
        if total < threshold {
            return Err(Error::InvalidThreshold);
        }
        env.storage().instance().set(&DataKey::SignerWeights, &weights);"""
text = text.replace(rem_old, rem_new)

# update_weight
update_weight = """    pub fn update_weight(env: Env, caller: Address, signer: Address, weight: u32) -> Result<(), Error> {
        Self::require_signer(&env, &caller)?;
        if weight == 0 { return Err(Error::InvalidInput); }
        let mut weights: Map<Address, u32> = env.storage().instance().get(&DataKey::SignerWeights).unwrap_or(Map::new(&env));
        if !weights.contains_key(signer.clone()) {
            return Err(Error::NotFound);
        }
        weights.set(signer.clone(), weight);
        let threshold = Self::threshold(&env)?;
        let mut total = 0;
        for (_, w) in weights.iter() {
            total += w;
        }
        if total < threshold {
            return Err(Error::InvalidThreshold);
        }
        env.storage().instance().set(&DataKey::SignerWeights, &weights);
        Self::bump_instance(&env);
        env.events().publish((symbol_short!("config"), symbol_short!("upd_wght")), (signer, weight));
        Ok(())
    }

    pub fn set_threshold"""
text = text.replace("    pub fn set_threshold", update_weight)

# set_threshold
thr_old = """    pub fn set_threshold(env: Env, caller: Address, threshold: u32) -> Result<(), Error> {
        Self::require_signer(&env, &caller)?;
        let len = Self::signers(&env)?.len();
        Self::validate_threshold(threshold, len)?;
        env.storage()"""
thr_new = """    pub fn set_threshold(env: Env, caller: Address, threshold: u32) -> Result<(), Error> {
        Self::require_signer(&env, &caller)?;
        let weights: Map<Address, u32> = env.storage().instance().get(&DataKey::SignerWeights).unwrap_or(Map::new(&env));
        let mut total = 0;
        for (_, w) in weights.iter() {
            total += w;
        }
        if threshold < MIN_THRESHOLD || threshold > total {
            return Err(Error::InvalidThreshold);
        }
        env.storage()"""
text = text.replace(thr_old, thr_new)

# approve
app_old = """        env.storage().persistent().set(&akey, &true);
        proposal.approvals = checked_add(proposal.approvals as i128, 1)? as u32;"""
app_new = """        env.storage().persistent().set(&akey, &true);
        let weights: Map<Address, u32> = env.storage().instance().get(&DataKey::SignerWeights).unwrap_or(Map::new(&env));
        let weight = weights.get(caller.clone()).unwrap_or(0);
        proposal.approvals = checked_add(proposal.approvals as i128, weight as i128)? as u32;"""
text = text.replace(app_old, app_new)

# signers internal view
signers_old = """    fn signers(env: &Env) -> Result<Vec<Address>, Error> {
        env.storage()
            .instance()
            .get(&DataKey::Signers)
            .ok_or(Error::NotInitialized)
    }"""
signers_new = """    fn signers(env: &Env) -> Result<Vec<Address>, Error> {
        let weights: Map<Address, u32> = env.storage().instance().get(&DataKey::SignerWeights).ok_or(Error::NotInitialized)?;
        let mut vec = Vec::new(env);
        for (k, _) in weights.iter() {
            vec.push_back(k);
        }
        Ok(vec)
    }"""
text = text.replace(signers_old, signers_new)

# require_signer internal view
req_old = """    fn require_signer(env: &Env, caller: &Address) -> Result<(), Error> {
        caller.require_auth();
        let signers = Self::signers(env)?;
        if !signers.contains(caller) {
            return Err(Error::NotASigner);
        }
        Ok(())
    }"""
req_new = """    fn require_signer(env: &Env, caller: &Address) -> Result<(), Error> {
        caller.require_auth();
        let weights: Map<Address, u32> = env.storage().instance().get(&DataKey::SignerWeights).ok_or(Error::NotInitialized)?;
        if !weights.contains_key(caller.clone()) {
            return Err(Error::NotASigner);
        }
        Ok(())
    }"""
text = text.replace(req_old, req_new)

with open("contracts/multisig/src/lib.rs", "w") as f:
    f.write(text)
