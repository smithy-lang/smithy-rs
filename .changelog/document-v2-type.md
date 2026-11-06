---
applies_to: ["client", "server", "aws-sdk-rust"]
authors: ["landonxjames"]
references: ["smithy-rs#4853"]
breaking: false
new_feature: true
bug_fix: false
---
Added `DocumentV2` to `aws-smithy-types`. `DocumentV2` represents the whole Smithy data model: it adds `Blob`, `Timestamp`, `BigInteger`, and `BigDecimal` variants, keeps object entries in insertion order with the new `DocumentObject` type, and is `#[non_exhaustive]`. `DiscriminatedDocument` is now generic over the document it wraps, with `Document` as the default: use `DiscriminatedDocument::new_v2` to wrap a `DocumentV2`, and the sealed `DocumentTrait` to write code that accepts either. `Document` is unchanged, and existing uses of `DiscriminatedDocument` keep their meaning. Generated code doesn't use the new types yet.
