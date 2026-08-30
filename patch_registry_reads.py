with open("contracts/registry/src/lib.rs", "r") as f:
    text = f.read()

# get_version
old = """    pub fn get_version(env: Env, kind: ModuleKind, version: u32) -> Result<Address, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Version(kind, version))
            .ok_or(Error::NotFound)
    }"""
new = """    pub fn get_version(env: Env, kind: ModuleKind, version: u32) -> Result<Address, Error> {
        let key = DataKey::Version(kind, version);
        let val = env.storage().persistent().get(&key).ok_or(Error::NotFound)?;
        Self::bump(&env, &key);
        Ok(val)
    }"""
text = text.replace(old, new)

# get_latest
old = """    pub fn get_latest(env: Env, kind: ModuleKind) -> Result<Address, Error> {
        let latest: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::LatestVersion(kind.clone()))
            .ok_or(Error::NotFound)?;
        Self::get_version(env, kind, latest)
    }"""
# Wait, let's see if DataKey::LatestVersion uses clone in the file
text = text.replace("""    pub fn get_latest(env: Env, kind: ModuleKind) -> Result<Address, Error> {
        let latest: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::LatestVersion(kind))
            .ok_or(Error::NotFound)?;
        Self::get_version(env, kind, latest)
    }""", """    pub fn get_latest(env: Env, kind: ModuleKind) -> Result<Address, Error> {
        let key = DataKey::LatestVersion(kind.clone());
        let latest: u32 = env.storage().persistent().get(&key).ok_or(Error::NotFound)?;
        Self::bump(&env, &key);
        Self::get_version(env, kind, latest)
    }""")

# get_org_owner
text = text.replace("""    pub fn get_org_owner(env: Env, org: String) -> Result<Address, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Org(org))
            .ok_or(Error::NotFound)
    }""", """    pub fn get_org_owner(env: Env, org: String) -> Result<Address, Error> {
        let key = DataKey::Org(org);
        let val = env.storage().persistent().get(&key).ok_or(Error::NotFound)?;
        Self::bump(&env, &key);
        Ok(val)
    }""")

# lookup
text = text.replace("""    fn lookup(env: Env, org: String, kind: ModuleKind) -> Result<Address, Error> {
        Self::check_frozen(&env)?;
        env.storage()
            .persistent()
            .get(&DataKey::Module(org, kind))
            .ok_or(Error::NotFound)
    }""", """    fn lookup(env: Env, org: String, kind: ModuleKind) -> Result<Address, Error> {
        Self::check_frozen(&env)?;
        let key = DataKey::Module(org, kind);
        let val = env.storage().persistent().get(&key).ok_or(Error::NotFound)?;
        Self::bump(&env, &key);
        Ok(val)
    }""")

# verify_owner
text = text.replace("""    fn verify_owner(env: Env, org: String, owner: Address) -> Result<bool, Error> {
        Self::check_frozen(&env)?;
        let recorded: Address = env
            .storage()
            .persistent()
            .get(&DataKey::Org(org))
            .ok_or(Error::NotFound)?;
        Ok(recorded == owner)
    }""", """    fn verify_owner(env: Env, org: String, owner: Address) -> Result<bool, Error> {
        Self::check_frozen(&env)?;
        let key = DataKey::Org(org);
        let recorded: Address = env.storage().persistent().get(&key).ok_or(Error::NotFound)?;
        Self::bump(&env, &key);
        Ok(recorded == owner)
    }""")

with open("contracts/registry/src/lib.rs", "w") as f:
    f.write(text)
