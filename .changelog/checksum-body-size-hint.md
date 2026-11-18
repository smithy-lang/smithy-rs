---
applies_to: ["client", "aws-sdk-rust"]
authors: ["patrick-immometrica"]
references: []
breaking: false
new_feature: false
bug_fix: true
---
Response bodies wrapped for checksum validation report the inner body's `size_hint` again, so `ByteStream::size_hint()` on e.g. S3 `GetObject` is no longer unknown.
