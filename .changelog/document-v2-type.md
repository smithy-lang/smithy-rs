---
applies_to: ["client", "server", "aws-sdk-rust"]
authors: ["landonxjames"]
references: ["smithy-rs#4853", "smithy-rs#5003"]
breaking: false
new_feature: true
bug_fix: false
---

Added `DocumentV2` to `aws-smithy-types`. `DocumentV2` represents the whole
Smithy data model. `Document` is unchanged.

- **Variants:** `DocumentV2` adds `Blob`, `Timestamp`, `BigInteger`, and
  `BigDecimal` variants, keeps object entries in insertion order with the new
  `DocumentObject` type, and is `#[non_exhaustive]`.
- **Conversions:** `From` converts a `Document` to a `DocumentV2` without loss.
  `DocumentV2::to_document_lossy` converts back, encoding blobs as base64
  strings, timestamps as epoch-seconds numbers, and arbitrary-precision numbers
  as numbers, as `Document` reads them from JSON.
  `DocumentV2::try_to_document` returns an error for those values instead.
- **Serde:** with `--cfg aws_sdk_unstable` and the `serde-serialize` or
  `serde-deserialize` feature, `DocumentV2` implements serde's traits.
  `document::to_document_v2` and `document::from_document_v2` convert between it
  and other serde types, like `to_document` and `from_document` do for
  `Document`.
- **`DiscriminatedDocument`:** it's now generic over the document it wraps, with
  `Document` as the default. Use `DiscriminatedDocument::new_v2` to wrap a
  `DocumentV2`, and the sealed `DocumentTrait` to write code that accepts either.
  Existing uses of `DiscriminatedDocument` keep their meaning.
