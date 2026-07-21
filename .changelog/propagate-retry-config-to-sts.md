---
applies_to:
- aws-sdk-rust
authors:
- ysaito1001
references: []
breaking: false
new_feature: false
bug_fix: false
---
Credential providers in the default chain now honor the outer `RetryConfig` and `TimeoutConfig`
when talking to remote endpoints (e.g., STS, SSO):

- **Retry:** the customer-configured `RetryConfig` (for example `AWS_MAX_ATTEMPTS`) is propagated
  to the inner STS client used by the default chain. Previously the inner STS client always used
  the default retry configuration (3 attempts), ignoring the outer setting. Now, setting
  `AWS_MAX_ATTEMPTS=5` causes the inner STS client to retry up to 5 times as well.
- **Timeout:** the customer-configured `TimeoutConfig` is likewise propagated to those inner
  clients (also settable directly via the new `ProviderConfig::with_timeout_config`).

IMDS and ECS providers are unaffected — they talk to local metadata endpoints and keep their own
retry/timeout configuration.

If you wish to opt out of the propagated behavior, pass a `ProviderConfig` with the desired
retry/timeout config to the default chain builder:

```rust
use aws_config::default_provider::credentials::DefaultCredentialsChain;
use aws_config::provider_config::ProviderConfig;
use aws_config::retry::RetryConfig;
use aws_config::timeout::TimeoutConfig;
use std::time::Duration;

let provider_config = ProviderConfig::default()
    // Pin credential-resolution retries independently of the service client
    // (use `RetryConfig::disabled()` to turn retries off entirely).
    .with_retry_config(RetryConfig::standard().with_max_attempts(3))
    // Optionally give the inner clients their own timeouts.
    .with_timeout_config(
        TimeoutConfig::builder()
            .operation_timeout(Duration::from_secs(5))
            .build(),
    );

let credentials_provider = DefaultCredentialsChain::builder()
    .configure(provider_config)
    .build()
    .await;

let sdk_config = aws_config::defaults(aws_config::BehaviorVersion::latest())
    .credentials_provider(credentials_provider)
    // other configurations
    .load()
    .await;
```
