---
applies_to: ["server"]
authors: ["Regalias3"]
references: ["smithy-rs#2363"]
breaking: true
new_feature: false
bug_fix: false
---
Server code generation now fails with a clear error when the service has no operations. This includes operations bound through resources. A server SDK with no operations cannot serve any request, so the build now stops instead of producing an unusable crate. To fix it, add at least one operation to the service.
