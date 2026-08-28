---
applies_to: ["client", "server", "aws-sdk-rust"]
authors: ["amodam-user"]
references: ["smithy-rs#4756"]
breaking: false
new_feature: true
bug_fix: false
---
`aws_smithy_types::Blob` now implements `Display` and renders its contents as a lowercase hex-encoded string in both `Display` and `Debug` output. Previously, the derived `Debug` implementation delegated to the underlying byte buffer, producing noisy `[u8, u8, ...]`-style output in service logs. Hex encoding keeps the payload legible without requiring the `@sensitive` trait for non-sensitive binary fields.
