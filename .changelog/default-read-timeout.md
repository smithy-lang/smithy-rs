---
applies_to: ["aws-sdk-rust"]
authors: ["yinychen"]
references: ["smithy-rs#5004"]
breaking: false
new_feature: true
bug_fix: false
---
AWS SDK clients can now apply a default `read_timeout`, bounding the wait from request dispatch until response headers arrive. It is off unless you set `AWS_ENABLE_DEFAULT_SOCKET_TIMEOUT_2026=true`, and is planned to become unconditional in a later release, at which point the variable is retired.

The value depends on the service: 5 minutes generally, 15 minutes for services that legitimately hold a connection open before replying, and none at all where any bound would break normal usage, such as S3. A `read_timeout` you configure yourself always takes precedence, and a read timeout failure is retried as a transient error.

This bounds only the wait for response headers, not a transfer that starts and then stalls — stalled stream protection already covers that.

One edge case when opting in: if you depend on an SDK crate with `default-features = false`, set no `sleep_impl`, and pin `BehaviorVersion` below `v2026_01_12`, client construction will fail asking for a `sleep_impl`. Enable `rt-tokio` or supply one.
