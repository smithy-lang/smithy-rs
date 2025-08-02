---
applies_to:
  - aws-sdk-rust
  - client
authors:
  - landonxjames
references:
  - smithy-rs#4208
breaking: false
new_feature: true
bug_fix: false
---
Add the ability to add `hints.mostly-unused = true` to Cargo.toml. Also enable this hint for the `aws-sdk-ec2`, `aws-sdk-s3`, and `aws-sdk-dynamodb` crates.
