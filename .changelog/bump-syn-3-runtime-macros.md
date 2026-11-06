---
applies_to: ["client", "server", "aws-sdk-rust"]
authors: ["chammika"]
references: ["smithy-rs#4998"]
breaking: false
new_feature: false
bug_fix: false
---
Upgraded `syn` from 2 to 3 in `aws-smithy-runtime-api-macros` and `aws-smithy-http-server-metrics-macro`, so dependents that already use syn 3 no longer compile syn twice.
