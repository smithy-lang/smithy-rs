---
applies_to: ["server"]
authors: ["adwsingh"]
references: []
breaking: false
new_feature: true
bug_fix: false
---

With `schemaSerde` enabled, server code generation now emits
`crate::type_registry::registry()`, a `TypeRegistry` with an entry for every
structure in the service closure, mirroring the client's `Client::registry()`.
Each entry carries the shape's schema and a deserializer that reads the shape
through its schema walker and validates its constraints, so code that only knows
a shape ID at runtime can resolve and deserialize the shape.
