---
applies_to: ["client", "server"]
authors: ["landonxjames"]
references: []
breaking: false
new_feature: true
bug_fix: false
---

Add `OperationSchema` to `aws-smithy-schema`, that relates an operation's shape
ID to its input, output, and error schemas and is built with a `const`
`OperationSchema::builder`, so it can be declared as a `static`. It also documents
`ShapeId::from_parts` as a supported constructor for shape IDs that borrow
runtime-owned strings.
