---
applies_to: ["client", "server", "aws-sdk-rust"]
authors: ["landonxjames"]
references: ["smithy-rs#4853"]
breaking: false
new_feature: true
bug_fix: false
---

Added `DocumentV2` to `aws-smithy-types`. `DocumentV2` represents the whole
Smithy data model: it adds `Blob`, `Timestamp`, `BigInteger`, and `BigDecimal`
variants, keeps object entries in insertion order with the new `DocumentObject`
type, and is `#[non_exhaustive]`. `From` converts a `Document` to a `DocumentV2`
without loss, and `DocumentV2::to_document_lossy` and
`DocumentV2::try_to_document` convert back: the first encodes blobs, timestamps,
and arbitrary-precision numbers as strings and numbers, and the second returns
an error for them. With `--cfg aws_sdk_unstable` and the `serde-serialize` or
`serde-deserialize` feature, `DocumentV2` implements serde's traits, and
`document::to_document_v2` and `document::from_document_v2` convert between it
and other serde types, like `to_document` and `from_document` do for `Document`.
`DiscriminatedDocument` is now generic over the document it wraps, with
`Document` as the default: use `DiscriminatedDocument::new_v2` to wrap a
`DocumentV2`, and the sealed `DocumentTrait` to write code that accepts either.
`Document` is unchanged, and existing uses of `DiscriminatedDocument` keep their
meaning. In `aws-smithy-schema`, the new `ShapeSerializer::write_document_v2`
and `ShapeDeserializer::read_document_v2` methods write and read a `DocumentV2`.
Their default implementations call `write_document` and `read_document`, so
existing implementations of the traits keep compiling, and a default write
returns an error for a value that `Document` can't represent.
`DocumentV2ShapeSerializer`, `DocumentV2ShapeDeserializer`, and the
`DiscriminatedDocumentV2Ext` trait (`from_struct_v2`, `as_shape_v2`, and
`shape_type_v2`) convert between typed shapes and
`DiscriminatedDocument<DocumentV2>`, keeping blobs, timestamps, and
arbitrary-precision numbers as native values and members in order. Numbers read
from a `DocumentV2` follow its accessors' rules, which truncate fractions.
`TypeRegistry::deserialize_document` accepts either wrapper, through the new
sealed `SchemaDocument` trait, and `DocumentShapeSerializer` stores a
`DocumentV2` value as its `to_document_lossy` conversion. In `aws-smithy-json`,
`JsonSerializer` and `JsonDeserializer` implement the new methods: objects keep
their member order, blobs are written as base64 strings, timestamps in the
codec's default format, and arbitrary-precision numbers as their exact text, and
a number read from JSON becomes a `BigInteger` or `BigDecimal` when no `u64`,
`i64`, or `f64` holds it exactly. `JsonSerializer::write_discriminated_document_v2`
and `JsonDeserializer::read_discriminated_document_v2` write and read the
`__type` discriminator. The CBOR, XML, and query codecs use the default methods
for now. Generated code doesn't use the new types yet.
