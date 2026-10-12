---
applies_to: ["client", "aws-sdk-rust"]
authors: ["aajtodd"]
references: []
breaking: false
new_feature: true
bug_fix: false
---
Connection-pool integration now supports a public `pool::HttpsBuilder` alias,
`Builder::has_explicit_partitions()`, and the documented `DriverSpawner::from_fn`
callback constructor without requiring `test-util`. These APIs let external
composition layers retain native transport configuration and supply connection
task placement.
