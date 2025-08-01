---
applies_to: ["client", "aws-sdk-rust"]
authors: ["joshtriplett"]
references: []
breaking: false
new_feature: true
bug_fix: false
---
Use new `hints.mostly-unused` in Cargo to speed up compilation, given that most
users of the AWS SDK crates will use a fraction of their API surface area. This
speeds up compilation of `aws-sdk-ec2` from ~4m07s to ~2m04s, a ~50% speedup.
