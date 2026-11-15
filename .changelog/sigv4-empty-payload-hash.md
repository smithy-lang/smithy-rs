---
applies_to: ["aws-sdk-rust"]
authors: ["FalkWoldmann"]
references: []
breaking: false
new_feature: false
bug_fix: false
---
SigV4 now uses the known SHA-256 of an empty payload instead of hashing it for every request without a body, such as GetObject or DeleteObject, and for every chunk of a signed streaming upload.
