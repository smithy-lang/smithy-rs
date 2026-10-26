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
