---
applies_to: ["aws-sdk-rust"]
authors: ["yychen23"]
references: ["smithy-rs#5004"]
breaking: false
new_feature: true
bug_fix: false
---
AWS SDK clients now apply a default `read_timeout`, under `BehaviorVersion::v2026_11_17` and later. It bounds the wait from request dispatch until response headers arrive, which is where an unresponsive peer would otherwise leave a request hanging until the operating system gives up.

The value depends on the service: 5 minutes generally, 15 minutes for services that legitimately hold a connection open before replying, and none at all where any bound would break normal usage, such as S3 — a large `PutObject` cannot produce response headers until the whole body has been sent.

This bounds only the wait for response headers, not a transfer that starts and then stalls — stalled stream protection already covers that, and remains on by default.

A `read_timeout` you configure yourself always takes precedence, whether it is longer, shorter, or disabled, and whether you set it on the client or per operation with `config_override`. Prefer setting a longer value over `set_read_timeout(None)`: if a disable is the only field your `TimeoutConfig` specifies, it also clears the 3.1 second connect timeout default, because a config with no field in the "set" state stops the merge from reading lower-priority layers. That behavior predates this change.

A read timeout failure is classified as a transient error and is retried, so a single unresponsive connection costs one attempt rather than the operation. To keep the previous behavior for every service, pin `BehaviorVersion` to an earlier version.
