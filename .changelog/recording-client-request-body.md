---
applies_to: ["client", "aws-sdk-rust"]
authors: ["iwang-1"]
references: ["smithy-rs#4173", "aws-sdk-rust#1310"]
breaking: false
new_feature: false
bug_fix: true
---
Fix `RecordingClient` hanging on requests with a body, such as DynamoDB requests, when wrapping a hyper 1.x connector.
