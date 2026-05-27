---
applies_to:
- aws-sdk-rust
- client
authors:
- PeterUlb
references: []
breaking: false
new_feature: false
bug_fix: true
---

Fix `ConnectorBuilder::default()` to enable `TCP_NODELAY` by default. Previously, the auto-derived `Default` impl left `enable_tcp_nodelay` at `false`, while the curated `Connector::builder()` initialized it to `true`. The `Default` impl on `ConnectorBuilder` is now hand-written to match `Connector::builder()`, so all construction paths, including the SDK's `default_https_client`, get `enable_tcp_nodelay = true` consistently. Without `TCP_NODELAY`, Nagle's algorithm can hold small writes in the kernel waiting for ACKs; on request shapes emitted as multiple small sub-MSS writes, such as the tested HTTP/2 small-body SDK path where HEADERS and DATA are flushed separately, this can add roughly one RTT plus delayed-ACK time. Callers who relied on the previous unintended `enable_tcp_nodelay = false` default of `ConnectorBuilder::default()` can restore that behavior with `.enable_tcp_nodelay(false)`.
