---
applies_to: ["client", "aws-sdk-rust"]
authors: ["sachinsharma3191"]
references: ["smithy-rs#4327"]
breaking: false
new_feature: false
bug_fix: true
---
Fix code generation for `@sparse` lists bound to `@httpQuery`. Previously the generated client code called the query formatter on the `Option<T>` element directly, which failed to compile. Sparse list elements are now unwrapped, serializing present values and skipping null entries.
