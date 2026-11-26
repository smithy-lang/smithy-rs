---
applies_to: ["client", "aws-sdk-rust"]
authors: ["Mattbusel"]
references: ["aws-sdk-rust#1216"]
breaking: false
new_feature: false
bug_fix: true
---
Required string members bound to endpoint parameters (`@contextParam`) no longer treat a whitespace-only value as unset. Requests such as S3 `GetObject` or `PutObject` with the key `" "` used to fail with `MissingField { field: "key" }` before they were sent. An empty string is still rejected.
