---
applies_to: ["server"]
authors: ["drganjoo"]
references: []
breaking: false
new_feature: true
bug_fix: false
---

Add `ServiceSchema` and `OperationSchema` descriptors to `aws-smithy-schema`, along with the
`ShapeType::Service` and `ShapeType::Operation` shape types. An `OperationSchema` references an
operation's shape schema (carrying operation-level traits such as `@http`), its input, output, and
error schemas; a `ServiceSchema` references the service shape, its version, its protocol trait IDs,
and its bound operations. Both are `const`-constructible so generated code can emit them as `static`
values. They are the foundation for schema-driven server serialization and routing.
