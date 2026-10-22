# Server protocol compatibility fixtures

These fixtures compare generated server serde with schema serde using the same
models, requests, protocol, and HTTP version.

- `codegen-server-test/custom-test-models/` contains the Smithy request, response,
  and malformed-request tests for body bindings, collection timestamp formats,
  and prefix headers. `codegen-server-test/build.gradle.kts` registers each model
  with schema serde enabled and disabled.
- `models/` contains the models used by the focused Kotlin compatibility tests
  and the broader source-driven comparison corpus.
- `cases/` contains HTTP request corpora for restJson1, restXml, awsJson1_0,
  awsJson1_1, and rpcv2Cbor.
- The `*.rs.in` files are Rust test templates inserted into generated crates by
  `SourceDrivenSchemaSerdeCompatibilityTest`.

`RestWireCompatibilityTest.kt` retains exact response byte and header checks
alongside the protocol tests. It also covers repeated headers, which cannot be
represented by the protocol test traits' single-value header maps. Sparse timestamp
null cases also stay in Kotlin because the Smithy protocol test validator currently
rejects null timestamp parameters even in sparse collections.

The models, corpora, and templates originated in `tools/schema-serde-compat`.
Tests now read these checked-in fixtures directly. No audit scripts are needed,
and default observation output goes into temporary directories.

`SCHEMA_COMPAT_ROOT` still selects an external checkout root and enables the
source-driven suite's external observation mode. Models, corpora, and templates
are read from `codegen-server/src/test/resources/protocol-compat/` in that checkout.
Runtime and observation-directory overrides keep their existing behavior.
