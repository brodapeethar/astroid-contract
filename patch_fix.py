import re
with open("contracts/multisig/src/lib.rs", "r") as f:
    text = f.read()

# Fix add_signer
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
        let mut weights: Map<Address, u32> = Map::new(&env);
        for s in signers.iter() {
            weights.set(s, 1);
        }
        env.storage().instance().set(&DataKey::SignerWeights, &weights);"""
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

# Fix remove_signer
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
        let mut weights: Map<Address, u32> = Map::new(&env);
        for s in signers.iter() {
            weights.set(s, 1);
        }
        env.storage().instance().set(&DataKey::SignerWeights, &weights);"""
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

with open("contracts/multisig/src/lib.rs", "w") as f:
    f.write(text)
