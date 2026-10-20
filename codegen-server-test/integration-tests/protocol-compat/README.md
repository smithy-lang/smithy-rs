# protocol-compat

Cross-protocol behavior tests over generated servers, in two styles:

- Behavior pinning: shared handlers mounted on generated services covering each
  protocol combination, schemaSerde on and off, driven by raw HTTP requests
  (`claim_order`, `routing`, `rpc_bindings`, `payloads`, `extractors`,
  `upgrade_failures`, `logging`, `api_surface`).
- Round trips: one server serving every built-in protocol (`src/multi.rs`),
  called over real sockets by a generated client per protocol (`clients`) and
  by one schema client selecting protocols at runtime (`runtime_protocol_clients`).
  Raw protocol requests exercise the same service (`wire`).

The service shares one set of handlers across AWS JSON 1.0/1.1, REST JSON, REST XML and RPCv2 CBOR.

`make test` generates the server, five protocol-specific clients, and the runtime-selecting schema client, then runs the client and wire suites. It also generates the service crates referenced by the other members of the integration-test workspace, which Cargo needs to load their manifests. The wire tests exercise the generated service directly without a listening socket and select REST XML explicitly by Content-Type.

The suites moved from `examples/multi-protocol` retain their assertions, including
Unit versus explicit empty structures, modeled errors, validation, and streams.
The runtime client suite also selects all five protocols against one listening server.
It uses the same generated `multi-protocol-client` SDK for every protocol, selecting
the protocol through `Config::builder().protocol(...)` or `set_protocol(...)`.
`make test` includes this suite; `make test-runtime-client` generates its dependencies
and runs only the runtime client suite.
A fresh run before the move recorded 23 passing and 10 failing runtime client tests,
plus seven passing wire tests. The existing client failures remain visible; they concern
AWS JSON empty-input routing and RPC event-stream initial data. Run these targets alone with:

```sh
cargo test --quiet -p protocol-compat --test runtime_protocol_clients --test wire --no-fail-fast
```

The runnable client showcase remains in `examples/multi-protocol`.

The shared multiprotocol models (common operation definitions, server fixtures, and single-protocol client services) live in `codegen-core/common-test-models/multi-protocol-compat/`; the remaining protocol-compat-only models live in `models/`. The server integration Gradle task generates both server and client crates before running the workspace.

The server model in `codegen-core/common-test-models/multi-protocol-compat/multi-protocol-server.smithy` imports the shared operations and the server extras. The extras include ordinary structure/union payloads with distinct member/target XML names, response status/header/prefix bindings, nested collections and union variants, blobs and timestamps, populated modeled errors, generated builder defaults, and absent/empty/null sparse values. JSON-only fuzz and coverage profiles additionally include DocumentValues; this operation is outside the all-five-protocol server. The generated clients retain their original operation sets; the added server operations are exercised by the wire tests.

The `scenario` input selects populated, absent, empty, default, override or error responses where applicable. REST carries it in `x-scenario`; RPC carries it as an ordinary body member. Tagged response fixtures drive the fuzz handlers, and request fixtures seed these selectors.

Native protocol tests assert the declared wire contract. A/B tests compare independently generated legacy and schema servers using the same model and inputs. The generic fuzz model has explicit reductions: see `tools/multi-protocol-ab-fuzz/README.md` and the generated model-projection manifests. A passing reduced-model comparison does not establish coverage for excluded operations or members.

Earlier investigation records are in `validation/multi-protocol-enrichment/` and `validation/multi-protocol-example/report.md`. Those records predate the subsequent document-null and XML union-list fixes.

## Reproducing the legacy baseline

`make mainline-baseline` records the shared scenarios against freshly generated
legacy servers with HTTP 1.x and `schemaSerde: false`. It exports the pinned
merge-base commit `06c62da902fba6059ab34d0dc3d570907560cd3a` into a temporary
directory and uses that source for both codegen and runtime. Models, handlers,
and scenarios are copied from this suite; pending runtime or codegen changes
cannot enter the oracle.

The recorder updates `baselines/mainline.json` only after generation and all
scenario replays succeed. It also writes `baselines/mainline.metadata.json`
with the source commit, settings, and input hashes. Generated code and build
logs remain in the printed temporary evidence directory. Python 3.12 or newer
is required.

To compare another local upstream commit, use
`make mainline-baseline MAINLINE_REF=<commit>`. Review the resulting baseline
diff before changing divergence pins; recording a baseline does not authorize
changing compatibility expectations.

## Selected-protocol extractor rejection tests

`extractor_rejections_legacy` runs generated services with `schemaSerde: false` and compares
schema services using the same model, protocol, configuration, and requests. It records all five
runtime rejection variants for all five built-in protocols, plus streaming restJson1
rejections. The comparison includes the full header map, exact body bytes, and
`RuntimeErrorExtension`. Additional cases preserve service-owned custom responses and response
extensions, verify handler dispatch, and distinguish required and optional request extensions.

The constants in `tests/support/extractor_rejections.rs` were established by running the generated
legacy fixtures. For example, a restJson1 validation rejection has status 400, JSON content type,
`x-amzn-errortype: ValidationException`, and the raw validation message as its body. The CBOR
runtime-validation fixture also returns the raw supplied bytes with its CBOR content type; the
tests deliberately preserve that observation.

`extractor_rejections_selected_protocol` verifies the shared `RuntimeError`,
`IntoResponse::into_response_with_protocol`, and `ServerProtocol::serialize_runtime_error` APIs
through generated services. Both this target and the legacy comparison target pass with the
runtime implementation.

The new target compares shared errors against generated legacy services, alternates protocols on
one service, tests every rejection-wrapper arity from two through eight and every failure position,
checks a service-owned context-aware override, and exercises both shared and custom streaming
rejections without polling their request bodies. It also checks the selected external protocol
serializer through the public middleware protocol-replacement seam, tuple-order independence,
all five built-in protocol-local conversions, and the shared error's context-free fallback.
A separate case registers an external protocol, routes a request with its router into a generated
operation's upgrade, and checks that its runtime-error serializer handles the rejection. Another
case sends each wire protocol to one service serving all five protocols.

Generate the existing fixtures with `make codegen`, then run either target from this directory:

```sh
cargo test --quiet -p protocol-compat --test extractor_rejections_legacy
cargo test --quiet -p protocol-compat --test extractor_rejections_selected_protocol
```

The Pokemon example is deferred. Existing protocol-specific error responses are compared against
the recorded legacy expectations; the new shared error's context-free fallback is tested separately.
