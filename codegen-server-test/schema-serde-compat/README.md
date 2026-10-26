# Server protocol compatibility fixtures

These fixtures compare generated server serde with schema serde using the same
models, requests, protocol, and HTTP version.

- `protocol-tests/` contains the Smithy request, response,
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

All tests read checked-in fixtures directly and write comparison output into
temporary directories. No external audit runner, runtime snapshot, or environment
switch is needed. The source-driven corpora retain only requests asserted by the
normal Kotlin suite; the larger historical observation corpus is not a regression
suite.

Response-binding tests generate into fresh temporary crates so unrelated models
in the shared Kotlin test workspace cannot affect dependency resolution.

The retired audit's useful assertions remain in these ordinary test entrypoints.
Kotlin names below are relative to
`codegen-server/src/test/kotlin/software/amazon/smithy/rust/codegen/server/smithy/`.

| Coverage | Permanent assertions |
| --- | --- |
| Five-protocol request/response comparisons | `protocols/SourceDrivenSchemaSerdeCompatibilityTest.kt`: handler values, no panic, status, all headers, parsed body, and exact wire bytes; ignores logs and normalizes REST JSON validation-error member order |
| Sensitive-model and XML-attribute compilation | The source-driven test pins known generated-serde compilation failures and requires schema compilation under the same warning policy |
| Sensitive serialization diagnostics | `rust-runtime/aws-smithy-http-server/tests/modeled_headers.rs` and `tests/sensitive_serialize_failure_log.rs`: assert marker redaction and the configured logging behavior |
| Trailing slash and label routing | `protocols/routing/TrailingSlashLabelsCompatibilityTest.kt`: complete legacy/schema captures with valid and rejected controls |
| Prefix-header whitespace and media encoding | `protocols/rest/PrefixHeaderWhitespaceCompatibilityTest.kt`, `PrefixHeaderMediaTypeCompatibilityTest.kt`, and `ResponsePrefixHeaderMediaTypeCompatibilityTest.kt`: decoded values, wire output, rejection, and the explicit empty-media-header exception |
| Duplicate/null JSON members and Document null | `protocols/json/DuplicateJsonNullCompatibilityTest.kt` and `protocols/restjson/DocumentNullCompatibilityTest.kt`: handler presence/value and complete response comparisons |
| Response bindings, modeled errors, AWS JSON response modes | `protocols/response/ServerResponseBindingsTest.kt`, `ModeledErrorContentTypeCompatibilityTest.kt`, and `protocols/awsjson/AwsJson10StrictCompatibilityTest.kt` / `AwsJson11StrictCompatibilityTest.kt` |
| Streaming blobs | `protocols/streaming/StreamingBlobHttp1GenerationTest.kt` asserts legacy output bytes; `RestStreamingBlobInputCompatibilityTest.kt`, `RpcV2CborStreamingBlobCompatibilityTest.kt`, and `StreamingBlobInputTest.kt` assert generated-server decoding, validation, buffering, and live REST upload behavior |
| Live HTTP/2 event streams | `codegen-server-test/integration-tests/eventstreams/tests/`: the same test source runs against generated serde and schema serde, each with enabled/disabled initial-response projections; checks initial data, missing required data, output messages, signatures, and timestamps |
| Document nesting (existing mainline coverage) | `rust-runtime/aws-smithy-json/src/deserialize/token.rs` and `src/codec/deserializer.rs`: clean depth errors, under-limit success, and custom limits; unchanged by this server PR |

The socket suite's shared source is
`codegen-server-test/integration-tests/eventstreams/tests/support/structured_eventstream_tests.rs`.
Its two test entrypoints select the serde implementation through crate aliases.
The normal `codegen-server-test:assemble` and `cargoTestIntegration` tasks generate
and exercise these projections. A generation-only Kotlin helper adds no assertions.

The depth observation helper likewise added no Kotlin compatibility assertions.
Existing mainline JSON tests already protect bounded Document parsing, including
arrays, objects, and acceptance below the limit. The audit's exact HTTP depth
matrix records a deferred compatibility difference rather than a server PR
regression to freeze. Its helper and orphaned model are removed without expanding
the JSON crate's tests. This does not certify identical HTTP error responses or
equal limits.

Historical compatibility decisions
----------------------------------

The scope reference for this cleanup is mainline revision
`10424bd8c126013544c7439c51d3aa4c9fc5cab1` (fresh pull on October 25, 2026).
The October 14–15, 2026 audit compared this development checkout's generated serde
with schema serde. Its original reference was **not** a clean mainline release.
The following records preserve decisions, not golden expectations for current
behavior. Future compatibility changes require a fresh mainline reproduction as
specified in `AGENTS.md`.

- **C06, duplicate members followed by null:** schema retaining the earlier scalar
  was accepted in the old audit. The permanent duplicate/null test instead asserts
  equality and expects generated serde to clear the scalar. Its October 25 cleanup
  run passed for all three JSON protocols, with matching schema captures. This is
  verified behavior in the development checkout, separate from the old observation
  and from a fresh mainline reproduction; the old note does not authorize weakening
  the equality assertion.
- **C08, numeric float/double overflow:** conversion to signed infinity was
  accepted, separately from integer behavior. The old observation reported
  generated-serde HTTP 400 versus schema HTTP 200 for `1e400`. This cleanup does
  not establish those current HTTP outcomes. Existing JSON codec numeric tests
  retain their own assertions; no new historical HTTP golden was added.
- **C09, decimal integer precision:** preserving `9007199254740993.0` exactly was
  accepted instead of reproducing generated serde's floating-point precision loss.
  The schema codec's integer tests assert exact conversion. The old cross-server
  handler difference remains a historical observation.
- **C10, Document depth:** the difference was deferred, with bounded parsing to
  remain in place. The old audit recorded numeric-leaf limits of 255 containers
  for generated serde, versus 126 in a schema structure and 127 at the schema root;
  empty-container leaves allowed one more container. These exact HTTP boundaries
  have not been reverified by this cleanup; existing parser guard tests do not
  declare the compatibility question resolved.

The media-prefix test also preserves its explicit compatibility exception: schema
rejects empty media header values rather than reproducing the generated-serde
panic. The streaming tests protect the current ordinary-input/ByteStream contract:
REST uploads remain live; RPC CBOR collects and validates before dispatch.
Earlier deferred streaming decoder/session APIs are superseded designs, not
requirements to restore.

Historical diagnostic captures, compilation snapshots, and large case counts
are not guarantees of compatibility or sensitive-data protection. Dedicated
runtime logging/redaction tests remain responsible for their asserted cases.
