---
applies_to: ["client", "server"]
authors: ["landonxjames"]
references: ["smithy-rs#4721"]
breaking: false
new_feature: true
bug_fix: false
---

`aws-smithy-schema`'s `TypeRegistry` can now hold runtime entries alongside the
static entries that generated code registers. A runtime entry implements the new
`RuntimeRegistration` trait. It owns or shares its schema and deserialization
context, so a registry can hold schemas built at runtime from a loaded model
without leaking them. Generated code and the existing registry APIs are
unchanged.

- **Lookup:** `resolve`, `resolve_fqn`, and `resolve_error_code` on
  `TypeRegistry` and `ComposedRegistry` return a `ResolvedEntry` for an entry of
  either kind, and `resolved_entries` iterates every entry. The existing
  `schema_for*`, `entry_for*`, and `iter` methods still return only static
  entries, because they promise `'static` schemas
- **Construction:** `TypeRegistryBuilder::insert_runtime`,
  `insert_shared_runtime`, and `extend_shared_runtime` index a registration by
  its shape ID without building its schema
- **Behavior:** `deserialize_document`, `reify_error`, and `compose` handle both
  kinds, and `len` and `is_empty` count both. A later insertion, or the argument
  of `compose`, replaces an entry with the same shape ID whatever the kinds of
  the two entries
