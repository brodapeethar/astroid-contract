with open("contracts/multisig/src/lib.rs", "r") as f:
    text = f.read()

thr_old = """    pub fn set_threshold(env: Env, caller: Address, threshold: u32) -> Result<(), Error> {
        Self::require_signer(&env, &caller)?;
        let signers = Self::signers(&env)?;
        Self::validate_threshold(threshold, signers.len())?;"""
thr_new = """    pub fn set_threshold(env: Env, caller: Address, threshold: u32) -> Result<(), Error> {
        Self::require_signer(&env, &caller)?;
        let weights: Map<Address, u32> = env.storage().instance().get(&DataKey::SignerWeights).unwrap_or(Map::new(&env));
        let mut total = 0;
        for (_, w) in weights.iter() {
            total += w;
        }
        if threshold < MIN_THRESHOLD || threshold > total {
            return Err(Error::InvalidThreshold);
        }"""
text = text.replace(thr_old, thr_new)

with open("contracts/multisig/src/lib.rs", "w") as f:
    f.write(text)
