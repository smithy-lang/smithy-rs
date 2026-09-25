---
applies_to: ["aws-sdk-rust"]
authors: ["landonxjames"]
references: []
breaking: false
new_feature: true
bug_fix: false
---

`aws-sdk-ssm` now uses schema-based serialization and deserialization instead of
the legacy per-shape `protocol_serde` code. SSM is the first AWS service on the
schema serde path as part of its phased rollout; no change in behavior is
expected. Other `awsJson1_1` services are unaffected.
