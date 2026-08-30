import re

with open("contracts/proposal/src/lib.rs", "r") as f:
    text = f.read()

# Add created_at and grace_period to Proposal struct
struct_old = """    pub approvals: u32,
    pub state: ProposalState,
    pub expires_at: u64,
}"""
struct_new = """    pub approvals: u32,
    pub state: ProposalState,
    pub created_at: u64,
    pub expires_at: u64,
    pub grace_period: u64,
}

impl Proposal {
    pub fn is_expired(&self, env: &Env) -> bool {
        self.expires_at != 0 && env.ledger().timestamp() >= self.expires_at
    }

    pub fn is_active(&self, env: &Env) -> bool {
        !self.is_expired(env) && matches!(self.state, ProposalState::Pending | ProposalState::Approved)
    }

    pub fn can_execute(&self, env: &Env) -> bool {
        self.is_active(env) && self.state == ProposalState::Approved
    }
}
"""
text = text.replace(struct_old, struct_new)

# Update create args
create_old = """        threshold: u32,
        expires_at: u64,
    ) -> Result<u64, Error> {"""
create_new = """        threshold: u32,
        expires_at: u64,
        grace_period: u64,
    ) -> Result<u64, Error> {"""
text = text.replace(create_old, create_new)

# Update create instantiation
inst_old = """            approvals: 0,
            state: ProposalState::Pending,
            expires_at,
        };"""
inst_new = """            approvals: 0,
            state: ProposalState::Pending,
            created_at: env.ledger().timestamp(),
            expires_at,
            grace_period,
        };"""
text = text.replace(inst_old, inst_new)

# Update approve
text = text.replace("Self::ensure_not_expired(&env, &proposal)?;", "if proposal.is_expired(&env) { return Err(Error::ProposalExpired); }")

# Update execute
text = text.replace("""        Self::ensure_not_expired(&env, &proposal)?;
        if caller != proposal.proposer {
            return Err(Error::Unauthorized);
        }
        if proposal.state != ProposalState::Approved {""", """        if !proposal.can_execute(&env) {
            return Err(Error::InvalidProposalState);
        }
        if caller != proposal.proposer {
            return Err(Error::Unauthorized);
        }""")

# Update expire
text = text.replace("""        if proposal.expires_at == 0 || env.ledger().timestamp() < proposal.expires_at {
            return Err(Error::InvalidProposalState);
        }""", """        if !proposal.is_expired(&env) {
            return Err(Error::InvalidProposalState);
        }""")

# Update cancel
text = text.replace("""        if matches!(
            proposal.state,
            ProposalState::Executed | ProposalState::Closed | ProposalState::Cancelled
        ) {
            return Err(Error::InvalidProposalState);
        }""", """        if matches!(
            proposal.state,
            ProposalState::Executed | ProposalState::Closed | ProposalState::Cancelled
        ) {
            return Err(Error::InvalidProposalState);
        }
        if proposal.grace_period != 0 && env.ledger().timestamp() > proposal.created_at + proposal.grace_period {
            return Err(Error::CancellationWindowClosed);
        }""")

# Remove ensure_not_expired function
text = re.sub(r"    /// Surface.*?fn ensure_not_expired.*?Ok\(\(\)\)\n    \}", "", text, flags=re.DOTALL)

with open("contracts/proposal/src/lib.rs", "w") as f:
    f.write(text)
