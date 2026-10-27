# protocol-compat

These tests compare generated serde (`schemaSerde: false`) with schema serde
(`schemaSerde: true`). Existing fixture names containing `legacy` identify generated
serde; HTTP 1.x fixtures use `aws-smithy-http-server`.

Cross-protocol behavior tests over generated servers, in two styles:

- Behavior pinning: shared handlers mounted on generated services covering each
  protocol combination, schemaSerde on and off, driven by raw HTTP requests
  (`routing`, `logging`, `api_surface`). Routing also covers protocol claim order and lost operation context.
  Extractor and middleware rejection suites pin protocol framing and streaming behavior.
- Round trips: one server serving every built-in protocol (`src/multi.rs`),
  called over real sockets by a generated client per protocol (`clients`) and
  by schema clients selecting protocols at runtime (`runtime_protocol_clients`).
  Raw protocol requests exercise the same service (`wire`).

The service shares one set of handlers across AWS JSON 1.0/1.1, REST JSON, REST XML and RPCv2 CBOR.

`make test` generates the server, five protocol-specific clients, and the runtime-selecting schema client, then runs the client and wire suites. It also generates the service crates referenced by the other members of the integration-test workspace, which Cargo needs to load their manifests. The wire tests exercise the generated service directly without a listening socket and select REST XML explicitly by Content-Type.

The suites moved from `examples/multi-protocol` retain their assertions, including
Unit versus explicit empty structures, modeled errors, validation, and streams.
The runtime client suite selects all five protocols through
`Config::builder().protocol(...)` or `set_protocol(...)`. Its structured-request
matrix uses the same REST-generated `multi-protocol-client` SDK and one listening
server for every protocol.

Empty requests and RPC streams use SDKs whose generated serializers support those
request shapes. AWS JSON empty requests use the SDK generated for the opposite
AWS JSON version; RPC event streams use the CBOR-generated SDK, including when
selecting AWS JSON at runtime. This preserves runtime protocol-switch coverage
without assuming that the REST-generated serializer can supply RPC request bodies
or event-stream initial frames. All 33 runtime-client tests retain their success,
error, and stream-content assertions.

`make test` includes this suite; `make test-runtime-client` generates its dependencies
and runs only the runtime client suite.
Run the runtime-client and wire targets alone with:

```sh
cargo test --quiet -p protocol-compat --test runtime_protocol_clients --test wire --no-fail-fast
```

The runnable client showcase remains in `examples/multi-protocol`.

The shared multiprotocol models (common operation definitions, server fixtures, and single-protocol client services) live in `codegen-server-test/custom-test-models/multi-protocol-compat/`; the remaining protocol-compat-only models live in `models/`. The server integration Gradle task generates both server and client crates before running the workspace.

The server model in `codegen-server-test/custom-test-models/multi-protocol-compat/multi-protocol-server.smithy` imports the shared operations and the server extras. The extras include ordinary structure/union payloads with distinct member/target XML names, response status/header/prefix bindings, nested collections and union variants, blobs and timestamps, populated modeled errors, generated builder defaults, and absent/empty/null sparse values. JSON-only fuzz and coverage profiles additionally include DocumentValues; this operation is outside the all-five-protocol server. The generated clients retain their original operation sets; the added server operations are exercised by the wire tests.

The `scenario` input selects populated, absent, empty, default, override or error responses where applicable. REST carries it in `x-scenario`; RPC carries it as an ordinary body member. Tagged response fixtures drive the fuzz handlers, and request fixtures seed these selectors.

Native protocol tests assert the declared wire contract. A/B tests compare independently generated serde and schema servers using the same model and inputs. The generic fuzz model has explicit reductions: see `tools/multi-protocol-ab-fuzz/README.md` and the generated model-projection manifests. A passing reduced-model comparison does not establish coverage for excluded operations or members.

Earlier investigation records are in `validation/multi-protocol-enrichment/` and `validation/multi-protocol-example/report.md`. Those records predate the subsequent document-null and XML union-list fixes.

## Generated SDK parity

`legacy_parity` sends the same shared request scenarios to the existing SDK pairs
generated from the same models with `schemaSerde: false` and `schemaSerde: true`.
It compares status, headers, and bodies directly, canonicalizing JSON object
member order while retaining exact CBOR encoding checks.

The only explicit differences are JSON number normalization for REST JSON and
AWS JSON 1.0, and the definite versus indefinite byte-string encoding of a CBOR
streaming blob. Those cases assert both SDKs' bodies and content lengths, then
require their remaining headers to match.

## Selected-protocol extractor rejection tests

`extractor_rejections_legacy` runs generated services with `schemaSerde: false` and compares
schema services using the same model, protocol, configuration, and requests. It records all five
runtime rejection variants for all five built-in protocols, plus streaming restJson1
rejections. The comparison includes the full header map, exact body bytes, and
`RuntimeErrorExtension`. Additional cases preserve service-owned custom responses and response
extensions, verify handler dispatch, and distinguish required and optional request extensions.

The constants in `tests/support/extractor_rejections.rs` were established by running the generated
generated serde fixtures. For example, a restJson1 validation rejection has status 400, JSON content type,
`x-amzn-errortype: ValidationException`, and the raw validation message as its body. The CBOR
runtime-validation fixture also returns the raw supplied bytes with its CBOR content type; the
tests deliberately preserve that observation.

`extractor_rejections_selected_protocol` verifies the shared `RuntimeError`,
`IntoResponse::into_response_with_protocol`, and `ServerProtocol::serialize_runtime_error` APIs
through generated services. Both this target and the generated serde comparison target pass with the
runtime implementation.

The new target compares shared errors against generated serde services, alternates protocols on
one service, tests every rejection-wrapper arity from two through eight and every failure position,
checks a service-owned context-aware override, and exercises both shared and custom streaming
rejections without polling their request bodies. It also checks tuple-order independence,
all five built-in protocol-local conversions, and the shared error's context-free fallback.
An external-protocol case registers the protocol, routes a request with its router into a generated
operation's upgrade, and checks that its runtime-error serializer handles the rejection. Another
case sends each wire protocol to one service serving all five protocols.

Generate the existing fixtures with `make codegen`, then run either target from this directory:

```sh
cargo test --quiet -p protocol-compat --test extractor_rejections_legacy
cargo test --quiet -p protocol-compat --test extractor_rejections_selected_protocol
```

The Pokemon example is deferred. Existing protocol-specific error responses are compared against
the recorded generated serde expectations; the new shared error's context-free fallback is tested separately.


## CI and coverage ownership

The `check-server-codegen-integration-tests` job in `.github/workflows/ci.yml`
runs `tools/ci-scripts/check-server-codegen-integration-tests`, which invokes
`./gradlew codegen-server-test:test`. Its `cargoTestIntegration` task generates
the fixtures and runs `cargo nextest run --no-tests=pass` in the integration
workspace. `protocol-compat` is a workspace member, so all its test targets run
in that job, including client round trips, direct generated-SDK parity,
and selected-protocol extractor and middleware rejections. The task also runs
the workspace doctests through `cargoTestIntegrationDoctests`.

The retained Kotlin checks run in the separate
`check-server-codegen-unit-tests` CI job, which invokes
`./gradlew codegen-server:test`.

Overlapping checks live in the following suites; these replacements exercise
generated services rather than only inspecting generated source.

| Behavior | Retained coverage |
| --- | --- |
| RPC response status/header bindings and historical/specification settings | Kotlin `ServerResponseBindingsTest`; direct on/off comparison in `legacy_parity` |
| Structured RPC payload member keys | Kotlin `ServerSchemaRpcPayloadTest`; `wire` and `legacy_parity` |
| Missing selected-operation context | `routing` covers registered handlers; Kotlin `MissingHandlerContextCompatibilityTest` covers missing handlers |
| Existing protocol-specific extractor API | `api_surface` |
| Streaming blob response content type | `wire` |

The client round trips and selected-protocol rejection suites retain their
existing assertions.
