---
applies_to: ["client", "server", "aws-sdk-rust"]
authors: ["FalkWoldmann"]
references: ["smithy-rs#3826"]
breaking: false
new_feature: true
bug_fix: false
---
Add a `convert-jiff-1-x` feature to `aws-smithy-types-convert` that provides `DateTimeExt::to_jiff` and `DateTimeExt::from_jiff` for converting between `DateTime` and `jiff::Timestamp`.
