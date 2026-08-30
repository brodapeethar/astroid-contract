import re

with open("contracts/wallet/src/lib.rs", "r") as f:
    text = f.read()

text = text.replace("use astroid_shared::math::{checked_add, checked_sub};", "use astroid_shared::math::{SafeAdd, SafeSub};")
text = text.replace("checked_add(count as i128, 1)? as u64", "(count as i128).safe_add(1)? as u64")
text = text.replace("checked_add(current, amount)?", "current.safe_add(amount)?")
text = text.replace("checked_sub(current, amount)?", "current.safe_sub(amount)?")

with open("contracts/wallet/src/lib.rs", "w") as f:
    f.write(text)
