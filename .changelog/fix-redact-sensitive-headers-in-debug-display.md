---
applies_to:
- aws-sdk-rust
- client
authors:
- aajtodd
references: []
breaking: false
new_feature: false
bug_fix: true
---

Improve the `Debug` output of HTTP `Headers` and `Request` in `aws-smithy-runtime-api` to redact values of headers commonly used to carry sensitive data. The header name remains visible and the value is replaced with a placeholder that includes the original byte length to preserve diagnostic utility. The `aws-sigv4` signer applies the same redaction when logging the canonical request. The plain `Display` impl on `CanonicalRequest` is unchanged to preserve the raw canonical form used by downstream consumers.
