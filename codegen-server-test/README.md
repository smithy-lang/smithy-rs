# Server Codegen Integration Tests

Refer to `../codegen-client-test/README.md` for documentation on how to use this
module. This module is analogous to the `codegen-client-test` one, but it's named
`codegen-server-test` and it runs the `rust-server-codegen` Smithy build
plugin instead.

The [request limits integration tests](integration-tests/request-limits/README.md)
check header deadlines, body size limits, default body-read deadlines, and
per-operation overrides against generated servers over TCP.
