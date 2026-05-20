---
applies_to:
- client
- server
- aws-sdk-rust
authors:
- dnorred
references: []
breaking: false
new_feature: true
bug_fix: false
---

Implement `serde::Serializer` and `serde::Deserializer` traits for `aws_smithy_types::Document`, allowing it to be used as a self-describing data format. This enables converting any `Serialize` type into a `Document` via `to_document()` and deserializing a `Document` into any `Deserialize` type via `from_document()`.
