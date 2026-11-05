---
applies_to: ["client", "aws-sdk-rust"]
authors: ["vcjana"]
references: ["smithy-rs#4718", "smithy-rs#4990"]
breaking: false
new_feature: true
bug_fix: false
---
Add `aws_smithy_observability::unit_of_work`, which tells a caller whether a response body was
validated against its checksum. Attach a `Capture` carrying a `checksums::ChecksumCapture` with
`.customize().interceptor(..)`, read the body, then call `try_outcome()`: it reads `Validated` with
the algorithm and computed checksum, `Mismatch` with both checksums, `NotValidated` with a reason,
or `Incomplete`. The SDK reports through a new stable contract in `aws-smithy-runtime-api`
(`client::unit_of_work::checksums`), so a capture keeps working when the caller and the SDK resolve
different releases of `aws-smithy-observability`. `aws-smithy-checksums` adds
`ChecksumBody::with_completion`.
