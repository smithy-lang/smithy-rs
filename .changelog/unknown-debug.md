---
applies_to: ["client"]
authors: ["landonxjames"]
references: ["smithy-rs#4137"]
breaking: false
new_feature: false
bug_fix: true
---

Fix bug with enum codegen

When the first enum generated has the `@sensitive` trait the opaque type
underlying the `UnknownVariant` inherits that sensitivity. This means that
it does not derive `Debug`. Since the module is only generated once this
causes a problem for non-sensitive enums that rely on the type deriving
`Debug` so that they can also derive `Debug`. We manually add `Debug` to
the module so it will always be there since the `UnknownVariant` is not
modeled and cannot be `@sensitive`.
