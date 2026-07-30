---
applies_to:
- client
- aws-sdk-rust
authors:
- vcjana
references:
- smithy-rs#4248
breaking: false
new_feature: true
bug_fix: false
---
Add 7 new client metrics to MetricsInterceptor: `smithy.client.call.attempts`, `smithy.client.call.errors`, `smithy.client.call.serialization_duration`, `smithy.client.call.deserialization_duration`, `smithy.client.call.auth.signing_duration`, `smithy.client.call.auth.resolve_identity_duration`, and `smithy.client.call.resolve_endpoint_duration`.
