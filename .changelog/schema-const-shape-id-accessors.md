---
applies_to: ["client", "server"]
authors: ["fahadzub"]
references: []
breaking: false
new_feature: true
bug_fix: false
---

Make `aws-smithy-schema`'s `ShapeId::as_str`, `ShapeId::namespace`, and
`ShapeId::shape_name` available in const expressions. This allows the server
runtime to convert schema shape IDs into its shape ID type at compile time.
