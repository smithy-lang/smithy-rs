---
applies_to: ["server"]
authors: ["adwsingh"]
references: []
breaking: false
new_feature: true
bug_fix: false
---

With `schemaSerde` enabled, server code generation now emits schema-driven
walkers for all structures and unions that have a document representation.
Walkers are no longer limited to shapes reachable from an operation input.
Shapes outside operation input deserialize directly into public types. Closed
enums and public constrained newtypes reject invalid values during conversion.
Other constraints are not validated for these shapes, which matches client
behavior.
