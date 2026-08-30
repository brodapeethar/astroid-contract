import re
with open("contracts/multisig/src/lib.rs", "r") as f:
    text = f.read()

# Update approve to use weight
approve_old = """        env.storage().persistent().set(&akey, &true);
        proposal.approvals = checked_add(proposal.approvals as i128, 1)? as u32;"""
approve_new = """        env.storage().persistent().set(&akey, &true);
        let weights: Map<Address, u32> = env.storage().instance().get(&DataKey::SignerWeights).unwrap_or(Map::new(&env));
        let weight = weights.get(caller.clone()).unwrap_or(0);
        proposal.approvals = checked_add(proposal.approvals as i128, weight as i128)? as u32;"""
text = text.replace(approve_old, approve_new)

# Add update_weight
update_weight = """    pub fn update_weight(env: Env, caller: Address, signer: Address, weight: u32) -> Result<(), Error> {
        Self::require_signer(&env, &caller)?;
        if weight == 0 { return Err(Error::InvalidInput); }
        let mut weights: Map<Address, u32> = env.storage().instance().get(&DataKey::SignerWeights).unwrap_or(Map::new(&env));
        if !weights.contains_key(signer.clone()) {
            return Err(Error::NotFound);
        }
        weights.set(signer.clone(), weight);
        let threshold = Self::threshold(&env);
        let mut total = 0;
        for (_, w) in weights.iter() {
            total += w;
        }
        if total < threshold {
            return Err(Error::InvalidThreshold);
        }
        env.storage().instance().set(&DataKey::SignerWeights, &weights);
        env.events().publish(
            (symbol_short!("config"), symbol_short!("upd_wght")),
            (signer, weight),
        );
        Ok(())
    }

    pub fn set_threshold"""
text = text.replace("    pub fn set_threshold", update_weight)

with open("contracts/multisig/src/lib.rs", "w") as f:
    f.write(text)
