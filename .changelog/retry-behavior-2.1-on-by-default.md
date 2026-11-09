---
applies_to: ["client", "aws-sdk-rust"]
authors: ["ysaito1001"]
references: ["smithy-rs#4965"]
breaking: false
new_feature: true
bug_fix: false
---
Retry Behavior 2.1, as described in [aws-sdk-rust#1431](https://github.com/awslabs/aws-sdk-rust/discussions/1431), is now on by default. Customers who want the previous retry behavior should pin `BehaviorVersion` to an older version, such as `v2026_01_12`.
