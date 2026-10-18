# protocol-compat

Cross-protocol behavior tests over generated servers, in two styles:

- Behavior pinning: shared handlers mounted on generated services covering each
  protocol combination, schemaSerde on and off, driven by raw HTTP requests
  (`claim_order`, `routing`, `rpc_bindings`, `payloads`, `extractors`,
  `upgrade_failures`, `logging`, `api_surface`).
- Round trips: one server serving every built-in protocol (`src/multi.rs`),
  called by a generated client per protocol over a real socket (`clients`), and
  by raw protocol requests (`wire`).

The service shares one set of handlers across AWS JSON 1.0/1.1, REST JSON, REST XML and RPCv2 CBOR.

`make test` generates the server and five clients, then runs the client and wire suites. It also generates the service crates referenced by the other members of the integration-test workspace, which Cargo needs to load their manifests. The wire tests exercise the generated service directly without a listening socket and select REST XML explicitly by Content-Type.

All multiprotocol models live in `models/`, including the common operation definitions, server fixtures, and single-protocol client services. The server integration Gradle task generates both server and client crates before running the workspace.

The server model in `models/multi-protocol-server.smithy` imports the shared operations and the server extras. The extras include ordinary structure/union payloads with distinct member/target XML names, response status/header/prefix bindings, nested collections and union variants, blobs and timestamps, populated modeled errors, generated builder defaults, and absent/empty/null sparse values. JSON-only fuzz and coverage profiles additionally include DocumentValues; this operation is outside the all-five-protocol server. The generated clients retain their original operation sets; the added server operations are exercised by the wire tests.

The `scenario` input selects populated, absent, empty, default, override or error responses where applicable. REST carries it in `x-scenario`; RPC carries it as an ordinary body member. Tagged response fixtures drive the fuzz handlers, and request fixtures seed these selectors.

Native protocol tests assert the declared wire contract. A/B tests compare independently generated legacy and schema servers using the same model and inputs. The generic fuzz model has explicit reductions: see `tools/multi-protocol-ab-fuzz/README.md` and the generated model-projection manifests. A passing reduced-model comparison does not establish coverage for excluded operations or members.

Earlier investigation records are in `validation/multi-protocol-enrichment/` and `validation/multi-protocol-example/report.md`. Those records predate the subsequent document-null and XML union-list fixes.
