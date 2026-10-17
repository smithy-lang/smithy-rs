---
applies_to: ["client", "aws-sdk-rust"]
authors: ["jasgin"]
references: []
breaking: false
new_feature: true
bug_fix: false
---

Add the `aws-smithy-client-metrics` crate, which records per-request metrics for smithy-rs clients and the AWS SDK and emits them through [metrique](https://github.com/awslabs/metrique). Install `ClientMetrics` on a client with `.interceptor(ClientMetrics::new())`. By default, SDK call fields are added to the current request's `MetricsPool`; repeated calls with the same service, qualifier and operation keep the last call. If no pool is in scope, the crate logs a rate-limited warning and writes a standalone entry instead. Field names follow the AWS SDK for Java 2.x where the same concept is recorded. The destination and missing-pool behavior can be configured separately. Calls that end in an attempt timeout, an operation timeout or cancellation still follow the configured routing policy, with `AttemptTimeouts`, `OperationTimeout` or `Cancelled` set.
