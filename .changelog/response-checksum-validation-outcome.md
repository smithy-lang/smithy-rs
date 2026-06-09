---
applies_to:
- client
- aws-sdk-rust
authors:
- aajtodd
references: []
breaking: false
new_feature: true
bug_fix: false
---
Response checksum validation results are now recorded on operation outputs. After consuming a response body, call `output.extensions().get::<aws_smithy_checksums::body::validate::ResponseChecksumValidationResult>()` to learn whether the body was validated against a checksum and, if so, which algorithm was used.
