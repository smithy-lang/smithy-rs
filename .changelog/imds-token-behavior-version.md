---
applies_to: ["aws-sdk-rust"]
authors: ["landonxjames"]
references: ["smithy-rs#4969"]
breaking: false
new_feature: false
bug_fix: true
---
The IMDS client now applies its configured behavior version to token requests as well as metadata requests. Before this fix, a client on a behavior version older than `v2026_11_10` still used Retry Behavior 2.1 for its token requests.
