---
applies_to: ["client", "aws-sdk-rust"]
authors: ["thomasqueirozb"]
references: ["smithy-rs#4484"]
breaking: false
new_feature: false
bug_fix: true
---
Clients for services with operations modeled with `@requestCompression` (such as `aws-sdk-cloudwatch`) no longer enable the `http-body-0-4-x` feature of `aws-smithy-types`. The request compression interceptor only uses `http-body` 1.x, so these clients no longer pull in `http` 0.2 and `http-body` 0.4.
