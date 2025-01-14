---
applies_to: ["client", "aws-sdk-rust"]
authors: ["landonxjames"]
references: ["smithy-rs#3845"]
breaking: false
new_feature: true
bug_fix: true
---

S3 client behavior is updated to always calculate a checksum by default for operations that support it (such as PutObject or UploadPart), or require it (such as DeleteObjects). The default checksum algorithm is CRC32. Checksum behavior can be configured using `when_supported` and `when_required` options - in shared config using request_checksum_calculation, or as env variable using AWS_REQUEST_CHECKSUM_CALCULATION.

The S3 client attempts to validate response checksums for all S3 API operations that support checksums. However, if the SDK has not implemented the specified checksum algorithm then this validation is skipped. Checksum validation behavior can be configured using `when_supported` and `when_required` options - in shared config using response_checksum_validation, or as env variable using AWS_RESPONSE_CHECKSUM_VALIDATION.
