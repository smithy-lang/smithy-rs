---
applies_to: ["client", "server", "aws-sdk-rust"]
authors: ["FalkWoldmann"]
references: ["smithy-rs#700"]
breaking: false
new_feature: false
bug_fix: false
---
Removed the `num-integer` dependency from `aws-smithy-types`. `DateTime` now uses `div_euclid` and `rem_euclid` from std, which give the same results here since all divisors are positive.
