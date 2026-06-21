---
applies_to:
- aws-sdk-rust
authors:
- lauzadis
references: []
breaking: false
new_feature: true
bug_fix: false
---

Add a `customizationConfig.awsSdk.endpointBasedAuthScheme.enabled` flag that opts a service into endpoint-based auth scheme resolution without requiring an addition to the built-in `EndpointBasedAuthSchemeAllowList`. The existing curated allowlist (`CloudFront KeyValueStore`, `EventBridge`, `S3`, `SESv2`) continues to work unchanged; the flag is additive and intended for services that need partition-specific auth resolution (e.g. sigv4a vs sigv4) driven by endpoint rules and cannot express that statically in the model. New AWS services should still prefer the static `auth` trait whenever possible.
