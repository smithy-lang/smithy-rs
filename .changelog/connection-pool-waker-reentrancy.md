---
applies_to: ["client", "aws-sdk-rust"]
authors: ["aajtodd"]
references: ["smithy-rs#4878"]
breaking: false
new_feature: false
bug_fix: true
---
Fix a possible deadlock in the opt-in connection pool when a custom async executor's waker re-enters the pool from its `clone` or `drop` callback. Tokio is not affected.
