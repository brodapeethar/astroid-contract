with open("contracts/multisig/src/lib.rs", "r") as f:
    text = f.read()

text = text.replace("env.storage().instance().set(&DataKey::SignerWeights, &signers);", """let mut weights: Map<Address, u32> = Map::new(&env);
        for s in signers.iter() {
            weights.set(s, 1);
        }
        env.storage().instance().set(&DataKey::SignerWeights, &weights);""")

with open("contracts/multisig/src/lib.rs", "w") as f:
    f.write(text)
