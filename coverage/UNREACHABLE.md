# Codegen code unreachable from any Smithy model

Lines here cannot be covered by adding models to the corpus. Whole-file entries are excluded
from the report via `coverageExclusions` in `build.gradle.kts`; partial-file entries still
count against the coverage percentage (JaCoCo cannot exclude line ranges), so 100% is not
attainable while this code exists.

## Excluded from the report (whole file unreachable)

| File | Reason |
|---|---|
| `**/testutil/**` (core, server, client) | Test-only infrastructure; never runs in a production codegen invocation. |
| `core/.../generators/SchemaGenerator.kt` | Schema-based serde is gated behind `SchemaSerdeAllowlist`, which is hardcoded empty (no setting can enable it). |
| `core/.../generators/SchemaTraitFilter.kt` | Same schema-serde gate. |
| `client/.../customize/ConditionalDecorator.kt` | Only instantiated by the AWS SDK plugin (`aws/codegen-aws-sdk`), out of scope for the generic plugins. |
| `client/.../customizations/DocsRsMetadataDecorator.kt` | Only instantiated by the AWS SDK plugin. |
| `server/.../customizations/AdditionalErrorsDecorator.kt` | Only enabled via a `META-INF/services` resource file, not reachable through models or settings. |

## Still in the report but (mostly) unreachable

| File | Unreachable portion | Reason |
|---|---|---|
| `client/.../protocol/RequestSerializerGenerator.kt` | ~140 lines (`schemaSerialize`, streaming/event-stream schema requests, `validateRequiredHttpBindings`) | Behind `SchemaSerdeAllowlist.usesSchemaSerdeExclusively` (hardcoded empty). |
| `client/.../protocol/ResponseDeserializerGenerator.kt` | ~154 lines (schema deserialize path) | Same schema-serde gate. |
| `client/.../customizations/SchemaDecorator.kt` | ~76 lines (customization bodies) | Same schema-serde gate; only the pass-through branches run. |
| `server/.../generators/ServiceConfigGenerator.kt` | ~125 lines (fallible-builder / required-config-method paths) | No decorator shipped with the stock `rust-server-codegen` plugin returns config methods; this is an extension point for downstream decorators. |
| `client/.../endpoint/rulesgen/StdLib.kt` | ~49 lines (`aws.parseArn`, `aws.isVirtualHostableS3Bucket`, `aws.partition`) | `awsStandardLib` is only registered by the AWS SDK plugin. |
| `server/.../ValidateUnsupportedConstraints.kt` | ~87 lines | The remaining branches are SEVERE messages that abort codegen (missing/multiple/conflicting validation exceptions, constrained event streams, maps under `@uniqueItems` lists, the "flag has no effect" complaint). A successful corpus cannot cover abort paths. |
| `server/.../validators/CustomValidationExceptionValidator.kt` | ~28 lines | All ERROR event branches for malformed `@validationException` shapes; emitting them fails the build. |
| `server/.../generators/ServerInstantiator.kt` | ~24 lines | `ServerBuilderKindBehavior` methods are never invoked because `ServerInstantiator` uses `InstantiatorConstructPattern.DIRECT`; `ServerBuilderInstantiator.setField` has no server-side caller in the stock plugin. |
| `server/.../PatternTraitEscapedSpecialCharsValidator.kt` | ~10 lines | ERROR path for invalid escaped chars in `@pattern`; aborts the build. |
| `server/.../RustCrateInlineModuleComposingWriter.kt` | ~20 of 36 lines | `createTestInlineModuleCreator` and `withInMemoryInlineModule` are test-support helpers living in main sources. |
| Various | `throw UnsupportedJmesPathException` / `InvalidJmesPathTraversal` / defensive `CodegenException` branches | Triggering them aborts the projection, which fails the whole coverage run; they can only be covered by unit tests, not by a successful codegen corpus. |

## Server-side accounting

Server codegen (`codegen-server`) stands at **93.1%** (7,009 / 7,527 lines). Of the 518
missed lines, ~280 are in the unreachable categories above, putting the practical ceiling
around **96–97%** and the achieved share of *coverable* lines at ~97%.

When the schema-serde allowlist is opened up (or the flag removed), the first three entries
become coverable and should get models.
