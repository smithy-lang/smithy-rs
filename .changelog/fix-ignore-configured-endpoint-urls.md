---
applies_to:
- aws-sdk-rust
authors:
- ysaito1001
references:
- smithy-rs#4730
breaking: false
new_feature: false
bug_fix: true
---

Fix `AWS_IGNORE_CONFIGURED_ENDPOINT_URLS` not suppressing service-specific endpoint URLs. Previously, the flag correctly suppressed the global `AWS_ENDPOINT_URL` but service-specific endpoint URLs (e.g. `AWS_ENDPOINT_URL_DYNAMODB` or `endpoint_url` in a `[services]` config section) bypassed the guard. Now all environment and config file endpoint URL sources are suppressed when the flag is set.
