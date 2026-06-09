---
applies_to: ["aws-sdk-rust", "client", "server"]
authors: ["iconara"]
references: ["aws-sdk-rust#1433", "aws-sdk-rust#1418"]
breaking: false
new_feature: false
bug_fix: true
---
Update the `User-Agent` header to contain the same information as the `x-amz-user-agent` header (including `AppName`, environment metadata, business metrics, etc.)
