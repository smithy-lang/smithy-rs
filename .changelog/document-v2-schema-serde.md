---
applies_to: ["client", "server", "aws-sdk-rust"]
authors: ["landonxjames"]
references: ["smithy-rs#4853", "smithy-rs#5007"]
breaking: false
new_feature: true
bug_fix: false
---

Schema serde can now write and read `DocumentV2` values.

- **Serializer and deserializer traits:** in `aws-smithy-schema`, the new
  `ShapeSerializer::write_document_v2` and `ShapeDeserializer::read_document_v2`
  methods write and read a `DocumentV2`. Their default implementations call
  `write_document` and `read_document`, so existing implementations of the
  traits keep compiling. A default write returns an error for a value that
  `Document` can't represent.
- **Typed shapes:** `DocumentV2ShapeSerializer`, `DocumentV2ShapeDeserializer`,
  and the `DiscriminatedDocumentV2Ext` trait (`from_struct_v2`, `as_shape_v2`,
  and `shape_type_v2`) convert between typed shapes and
  `DiscriminatedDocument<DocumentV2>`. They keep blobs, timestamps, and
  arbitrary-precision numbers as native values, and members in order. Numbers
  read from a `DocumentV2` follow its `coerce_*` methods' rules, which truncate
  fractions.
- **Type registries:** `TypeRegistry::deserialize_document` accepts either
  wrapper, through the new sealed `SchemaDocument` trait.
  `DocumentShapeSerializer` stores a `DocumentV2` value as its
  `to_document_lossy` conversion.
- **Union decode mode:** `DocumentUnionDecodeMode` selects whether a generated
  client union reads a member that holds documents into its original variant
  (`V1`, the default) or its `DocumentV2` variant (`V2`). The new
  `ShapeDeserializer::document_union_decode_mode` method reports the mode and
  returns `V1` by default, so existing implementations keep compiling.
  `DocumentUnionDecodeMode::read_with` makes every value a read reaches report
  the mode, without changes to the deserializers it reads through.
- **JSON:** in `aws-smithy-json`, `JsonSerializer` and `JsonDeserializer`
  implement the new methods. Objects keep their member order. Blobs are written
  as base64 strings, timestamps in the codec's default format, and
  arbitrary-precision numbers as their exact text. A number read from JSON
  becomes a `BigInteger` or `BigDecimal` when no `u64`, `i64`, or `f64` holds it
  exactly. `JsonSerializer::write_discriminated_document_v2` and
  `JsonDeserializer::read_discriminated_document_v2` write and read the `__type`
  discriminator.
- **CBOR, XML, and Query:** the `aws-smithy-cbor`, `aws-smithy-xml`, and
  `aws-smithy-query` serializers reject every `DocumentV2` value with the same
  error as a `Document`, because their protocols have no document encoding.
