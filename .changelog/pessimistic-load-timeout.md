---
applies_to:
- client
- aws-sdk-rust
authors:
- ysaito1001
references: []
breaking: false
new_feature: false
bug_fix: false
---
Replace the hardcoded 5-second identity cache `load_timeout` with a pessimistic timeout derived from the configured `RetryConfig` and `TimeoutConfig`. The new default ensures the inner credential provider's retry strategy has enough time to exhaust all configured attempts before the cache kills the resolution future. With default settings (3 attempts, 3.1s connect timeout), the derived timeout is approximately 22 seconds. Customers who explicitly set `load_timeout` are unaffected.

If you rely on the legacy 5-second timeout behavior, you can restore it explicitly:

```rust
use aws_smithy_runtime::client::identity::IdentityCache;
use std::time::Duration;

IdentityCache::lazy()
    .load_timeout(Duration::from_secs(5))
    .build()
```
