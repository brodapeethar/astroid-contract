import re

with open("contracts/proposal/src/test.rs", "r") as f:
    text = f.read()

# Fix the try_create calls missing the 9th argument
text = text.replace("""        &5_000,
    );""", """        &5_000,
        &0,
    );""")

text = text.replace("""        &500, // in the past (now = 1000)
    );""", """        &500, // in the past (now = 1000)
        &0,
    );""")

text = text.replace("""        &expires_at,
    )""", """        &expires_at,
        &0,
    )""")

# Fix &h.approvers -> &approver_vec(&h)
text = text.replace("&h.approvers,", "&approver_vec(&h),")

with open("contracts/proposal/src/test.rs", "w") as f:
    f.write(text)
