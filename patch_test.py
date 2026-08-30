import re
with open("contracts/multisig/src/test.rs", "r") as f:
    text = f.read()

text = re.sub(r"h\.client\.add_signer\(&h\.signers\[0\], &new_signer\);", r"h.client.add_signer(&h.signers[0], &new_signer, &1);", text)
text = re.sub(r"h\.client\.try_add_signer\(&h\.signers\[0\], &h\.signers\[1\]\);", r"h.client.try_add_signer(&h.signers[0], &h.signers[1], &1);", text)
text = re.sub(r"h\.client\.try_add_signer\(&stranger, &extra\)", r"h.client.try_add_signer(&stranger, &extra, &1)", text)

with open("contracts/multisig/src/test.rs", "w") as f:
    f.write(text)
