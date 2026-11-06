---
applies_to: ["aws-sdk-rust"]
authors: ["yinychen"]
references: ["smithy-rs#4971"]
breaking: false
new_feature: true
bug_fix: false
---
AWS SDK clients now apply a default read timeout, under `BehaviorVersion::v2026_11_15` and later. The read timeout bounds the span from request dispatch until response headers arrive, which is where an unresponsive peer would otherwise leave a request hanging until the operating system gives up.

The value depends on the service: 5 minutes generally, 15 minutes for services that legitimately hold a connection open before replying, and no timeout for services where any bound would break normal usage. A large `PutObject`, for instance, cannot produce response headers until the whole body has been sent, so S3 applies no read timeout.

A `read_timeout` you configure yourself always takes precedence, whether it is longer, shorter, or disabled, and whether you set it on the client or per operation with `config_override`. To raise the limit rather than remove it:

```rust
let config = aws_sdk_dynamodb::Config::builder()
    .timeout_config(
        TimeoutConfig::builder()
            .read_timeout(Duration::from_secs(600))
            .build(),
    )
    .build();
```

Prefer setting a longer value over calling `set_read_timeout(None)`. Disabling a single timeout currently also clears the 3.1 second connect timeout default, because a `TimeoutConfig` with no field in the "set" state stops the config merge from reading lower-priority layers. That behavior predates this change.

To keep the previous behavior for every service, pin to an earlier behavior version.
