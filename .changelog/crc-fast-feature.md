---
applies_to: ["client", "aws-sdk-rust"]
authors: ["landonxjames"]
references: ["aws-sdk-rust#1291"]
breaking: false
new_feature: false
bug_fix: true
---

Removing the `optimize_crc32_auto` feature flag from the `crc-fast` dependency of the `aws-smithy-checksums` crate since it was causing build issues for some customers.
