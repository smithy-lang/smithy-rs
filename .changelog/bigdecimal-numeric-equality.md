---
applies_to: ["client", "server", "aws-sdk-rust"]
authors: ["Regalias3"]
references: ["smithy-rs#4863"]
breaking: false
new_feature: false
bug_fix: true
---
`BigDecimal` now compares and hashes by numeric value instead of by spelling. `"1e2"` equals `"100"`, `"1.0"` equals `"1.00"`, `"+5"` equals `"5"`, and every spelling of zero, such as `"-0.0"` and `"0"`, is equal. `AsRef<str>` still returns the original string, so serialization doesn't change.
