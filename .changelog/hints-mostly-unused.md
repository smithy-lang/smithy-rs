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
Add the ability to insert `hints.mostly-unused = true` in Cargo.toml. Enable this hint for the below crates:
- aws-sdk-cloudformation
- aws-sdk-dynamodb
- aws-sdk-ec2
- aws-sdk-s3
- aws-sdk-sns
- aws-sdk-sqs
- aws-sdk-ssm
- aws-sdk-sts

See more information about this hint at https://blog.rust-lang.org/inside-rust/2025/07/15/call-for-testing-hint-mostly-unused/
