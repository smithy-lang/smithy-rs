# Server Codegen Integration Tests

Refer to `../codegen-client-test/README.md` for documentation on how to use this
module. This module is analogous to the `codegen-client-test` one, but it's named
`codegen-server-test` and it runs the `rust-server-codegen` Smithy build
plugin instead.

Handwritten Rust integration tests live in `integration-tests/`. The `eventstreams`
and `eventstreams-legacy` crates exercise generated services over HTTP/2. The
`multi-protocol` crate tests one server using generated clients for each built-in
protocol, and also sends requests directly through its handlers.

Run `./gradlew :codegen-server-test:cargoTestIntegration` from the repository root
to generate the required server and client crates and run this workspace.
For the multiprotocol suite alone, run `make test` in
`codegen-server-test/integration-tests/protocol-compat`.

Server-only protocol regressions live in `custom-test-models/` and run through
the normal protocol-test generator. The body-binding, collection-timestamp,
and prefix-header fixtures run with schema serde enabled and disabled on
HTTP 1.x. The RPC response-binding fixtures also run generated serde on
HTTP 0.x, preserving the historical AWS JSON status/header behavior and
CBOR's body-only bindings.

The multi-protocol service has paired AWS JSON response fixtures and projections
for historical and `smithy-specs` behavior. See
[the fixture guide](custom-test-models/multi-protocol-compat/README.md) for the
settings, recorded legacy responses, and commands to run both modes.

Response cases can opt into checking the complete set of header names using
Smithy's vendor parameters:

```smithy
vendorParamsShape: "smithy.rust.test#ResponseTestOptions",
vendorParams: {
    forbidAdditionalHeaders: true
}
```

Import `custom-test-models/server-response-test-options.smithy` alongside the
fixture to validate these parameters. When enabled, the allowed names are the
union of `headers` and `requireHeaders`, compared case-insensitively. An empty
union requires an empty response header map. Ordinary header value, presence,
and `forbidHeaders` checks still apply; `forbidHeaders: ["*"]` retains its
literal meaning.

The RPC fixtures compare JSON and CBOR response bodies semantically. When a
response case declares `content-length` in `headers` or `requireHeaders` and
defines a body, the generated test also checks that the actual length matches
the emitted body bytes. Requiring that header without specifying its value
permits equivalent encodings of different lengths.

Specialized codegen regressions retain their models, request corpora, and
Rust test templates in `codegen-server/src/test/resources/protocol-compat/`.
These cover additional behavior such as repeated physical headers, sparse
timestamp nulls, settings, streaming, and compilation failures.
