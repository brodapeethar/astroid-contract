with open("contracts/registry/src/lib.rs", "r") as f:
    text = f.read()

# register_org
text = text.replace("""        env.events().publish(
            (symbol_short!("org"), symbol_short!("register")),
            (org, owner),
        );""", """        env.events().publish(
            (symbol_short!("org"), symbol_short!("register"), org.clone()),
            owner,
        );""")

# transfer_org_ownership
text = text.replace("""        env.events().publish(
            (symbol_short!("org"), symbol_short!("owner")),
            (org, new_owner),
        );""", """        env.events().publish(
            (symbol_short!("org"), symbol_short!("owner"), org.clone()),
            new_owner,
        );""")

# register_module
text = text.replace("""        env.events().publish(
            (symbol_short!("module"), symbol_short!("register")),
            (org, kind, address),
        );""", """        env.events().publish(
            (symbol_short!("module"), symbol_short!("register"), org.clone(), kind.clone()),
            address,
        );""")

# remove_module
text = text.replace("""        env.events().publish(
            (symbol_short!("module"), symbol_short!("remove")),
            (org, kind),
        );""", """        env.events().publish(
            (symbol_short!("module"), symbol_short!("remove"), org.clone(), kind.clone()),
            (),
        );""")

# register_version
text = text.replace("""        env.events().publish(
            (symbol_short!("version"), symbol_short!("register")),
            (kind, version, address),
        );""", """        env.events().publish(
            (symbol_short!("version"), symbol_short!("register"), kind.clone(), version),
            address,
        );""")

with open("contracts/registry/src/lib.rs", "w") as f:
    f.write(text)
