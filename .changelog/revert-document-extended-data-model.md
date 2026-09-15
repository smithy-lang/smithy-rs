---
applies_to: ["client", "server", "aws-sdk-rust"]
authors: ["landonxjames"]
references: ["smithy-rs#4721", "smithy-rs#4837"]
breaking: true
new_feature: false
bug_fix: true
---
Reverted the expanded `aws_smithy_types::Document` data model and the accompanying type/error registries, restoring the `Document` API as it was in `aws-smithy-types` 1.6.3.

The expanded `Document` was a source-breaking change shipped as a minor version bump (`aws-smithy-types` 1.6.3 -> 1.7.0). Every published `aws-smithy-json` before 0.64.0 declares an open-ended `aws-smithy-types = "^1.x"` requirement while matching exhaustively on `Document` and constructing `Document::Object(HashMap)`, so those crates stopped compiling as soon as cargo resolved `aws-smithy-types` to 1.7.0. Because `aws-config` 1.12.0 still required `aws-smithy-json ^0.63.0`, a clean `cargo add aws-config` resolved both json 0.63.0 and 0.64.0 against a single unified `aws-smithy-types` 1.7.0 and failed to build. The 1.1.7 runtime release has been yanked in full.

**What this restores**

- `Document` returns to six variants (`Object`, `Array`, `Number`, `String`, `Bool`, `Null`) and is no longer `#[non_exhaustive]`, so exhaustive `match` statements compile again without a wildcard arm.
- `Document::Object` holds a `HashMap<String, Document>` again. `DocumentObject` is removed, along with its insertion-order iteration guarantee; object iteration order is unspecified once more.
- The companion public types `DiscriminatedDocument`, `DocumentSettings`, and `DocumentError` are removed, as are the `as_blob` / `as_timestamp` / `as_big_integer` / `as_big_decimal` accessors and the `Result`-returning numeric accessors.
- `aws_smithy_schema`'s `ShapeId` and `Schema` drop their lifetime parameter, returning to `ShapeId` / `Schema` with `ShapeId::from_static(...)`.
- The `TypeRegistry` and error-registry machinery is removed, including the generated package-level `registry()` and `error_registry()` accessors and the registry-backed fallback for unmodeled error codes. Error dispatch returns to the generated `match error_code { ... }` over each operation's modeled errors.
- Schema-based serde is no longer enabled for the SSM client.

**Migration**

Code written against 1.6.3 needs no changes. Code that adopted the 1.7.0 `Document` should revert to the 1.6.3 shape: drop wildcard `match` arms added solely for `#[non_exhaustive]`, replace `DocumentObject` annotations with `HashMap<String, Document>`, and stop relying on insertion-ordered object iteration. Since 1.7.0 is yanked, cargo will resolve `^1` to the restored API automatically.
