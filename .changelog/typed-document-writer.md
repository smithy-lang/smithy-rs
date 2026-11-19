---
applies_to: ["client", "server", "aws-sdk-rust"]
authors: ["landonxjames"]
references: ["smithy-rs#4853"]
breaking: false
new_feature: true
bug_fix: false
---

The new `TypeRegistry::serialize_document` writes a discriminated document as
the shape its discriminator names. It reads the document into that shape, then
passes the value to the new `ShapeSerializer::write_typed_document`, so the
protocol writes the members by the shape's schema, with traits such as
`@jsonName` and `@timestampFormat`, and writes the discriminator by its own
convention. The JSON codec writes the discriminator as a leading `__type`
member, and so writes the same bytes as for the typed value plus `__type`. A
document built with `DiscriminatedDocument::from_struct` or `from_struct_v2`
can now be sent over restJson1 and read back. The default
`write_typed_document` returns an unsupported-operation error, so protocols
without document support keep rejecting documents. Registry entries opt in
with the new `RegistryEntry::with_serializable`, and the registries of clients
generated with schema serde set it on every entry.
