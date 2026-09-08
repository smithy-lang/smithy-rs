---
applies_to: ["server"]
authors: ["drganjoo"]
references: []
breaking: false
new_feature: true
bug_fix: false
---

Add the `schemaSerde` server codegen setting. When enabled, every generated structure and union
exposes its `aws-smithy-schema` schema through a `SCHEMA` constant, and the crate gains
crate-private `schema::operations` and `schema::service` modules holding `OperationSchema` and
`ServiceSchema` descriptors for the service and its operations. The setting is off by default and
does not change generated code or runtime behavior when left off.
