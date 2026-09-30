---
applies_to: ["client", "aws-sdk-rust"]
authors: ["aajtodd"]
references: ["smithy-rs#4808", "smithy-rs#4824", "smithy-rs#4831", "smithy-rs#4864", "smithy-rs#4865", "smithy-rs#4866", "smithy-rs#4876"]
breaking: false
new_feature: true
bug_fix: false
---

Add an opt-in Hyper 1.x connection pool with HTTP/1.1 and HTTP/2 support, bounded per-origin connections, configurable partitions, and connection telemetry. The existing default HTTP client is unchanged.
