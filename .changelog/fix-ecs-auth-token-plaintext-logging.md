---
applies_to:
- aws-sdk-rust
authors:
- aajtodd
references: []
breaking: false
new_feature: false
bug_fix: true
---

Redact the `AWS_CONTAINER_AUTHORIZATION_TOKEN` value from WARN log output and error `Display` output when the ECS/EKS container credential provider rejects the token as invalid for use as an HTTP header. Previously, if the token contained a byte rejected by `HeaderValue` validation, it was logged at WARN level and embedded in the error string propagated through the credential chain. The value is now redacted.
