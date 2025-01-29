---
applies_to: ["aws-sdk-rust"]
authors: ["landonxjames"]
references: ["smithy-rs#3991"]
breaking: false
new_feature: false
bug_fix: true
---

Exclude `transfer-encoding` header from sigv4(a) signing since it is a hop by hop header that can be modified or removed by a proxy.
