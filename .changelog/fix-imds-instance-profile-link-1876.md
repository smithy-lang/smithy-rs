# Fix broken instance profile link in aws-config IMDS docs
---
applies_to: ["aws-sdk-rust"]
authors: ["hippi345"]
references: ["smithy-rs#1876"]
breaking: false
new_feature: false
bug_fix: true
---
Fix broken instance profile documentation link in `aws-config::imds::credentials::Builder::profile` by changing from inline link syntax to reference-style link syntax.
