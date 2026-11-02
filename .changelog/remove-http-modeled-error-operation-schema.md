---
applies_to: ["server"]
authors: ["fahadzub"]
references: []
breaking: true
new_feature: false
bug_fix: true
---

Fix modeled-error serialization incorrectly using the operation's success response schema to determine the default content type. Errors use their own schema and the protocol's error content type, so `HttpModeledError` no longer needs operation context. Explicit error content-type headers still take precedence.
