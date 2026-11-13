---
applies_to: ["client", "server", "aws-sdk-rust"]
authors: ["landonxjames"]
references: ["smithy-rs#4853"]
breaking: false
new_feature: false
bug_fix: true
---

Reading a typed shape from a document read over a JSON protocol that honors
`@jsonName`, such as restJson1, now matches the document's keys to members the
way the protocol does. Before, `TypeRegistry::deserialize_document`,
`DiscriminatedDocumentExt::as_shape`, and `DiscriminatedDocumentV2Ext::as_shape_v2`
matched every key against Smithy member names, so they skipped the members that
`@jsonName` renames. The new `DocumentSettings::use_json_name` method tells
them which names a document uses. It defaults to `false`, and `JsonCodecSettings`
returns its `use_json_name` setting. A document built from a typed shape, such
as with `DiscriminatedDocument::from_struct`, has no settings and still uses
member names.
