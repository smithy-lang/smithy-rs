---
applies_to:
- client
authors:
- aajtodd
references:
- smithy-rs#4767
breaking: false
new_feature: true
bug_fix: false
---
Add a deterministic connection-level test harness to the `wire-mock` feature under `aws_smithy_http_client::test_util::wire::connection`. It supports per-connection HTTP/1.1 scripts, raw socket actions, synchronization gates, and recorded connection events.
