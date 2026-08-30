import re
with open("contracts/multisig/src/test.rs", "r") as f:
    text = f.read()

text = text.replace("client.add_signer(&h.signers[0], &s3);", "client.add_signer(&h.signers[0], &s3, &1);")
text = text.replace("h.client.try_add_signer(&h.signers[0], &h.signers[0])", "h.client.try_add_signer(&h.signers[0], &h.signers[0], &1)")
text = text.replace("h.client.try_add_signer(&h.signers[0], &new)", "h.client.try_add_signer(&h.signers[0], &new, &1)")
text = text.replace("client.add_signer(&s1, &s3);", "client.add_signer(&s1, &s3, &1);")

test_code = """
#[test]
fn test_dynamic_weights_and_threshold() {
    let h = setup(3, 3);
    let s1 = &h.signers[0];
    let s2 = &h.signers[1];
    let s3 = &h.signers[2];
    
    // total weight is 3. Try to set threshold to 4, should fail.
    assert_eq!(h.client.try_set_threshold(s1, &4), Err(Ok(Error::InvalidThreshold)));
    
    // Try to set threshold to 0, should fail.
    assert_eq!(h.client.try_set_threshold(s1, &0), Err(Ok(Error::InvalidThreshold)));
    
    // update weight of s1 to 2. Total is 4.
    h.client.update_weight(s1, s1, &2);
    
    // now we can set threshold to 4
    h.client.set_threshold(s1, &4);
    
    // try to remove s3. weight would drop to 3, but threshold is 4.
    assert_eq!(h.client.try_remove_signer(s1, s3), Err(Ok(Error::InvalidThreshold)));
    
    // Try to update s1 weight to 1. total drops to 3.
    assert_eq!(h.client.try_update_weight(s1, s1, &1), Err(Ok(Error::InvalidThreshold)));
}
"""
text += test_code

with open("contracts/multisig/src/test.rs", "w") as f:
    f.write(text)
