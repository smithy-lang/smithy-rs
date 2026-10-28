# Server Codegen Integration Tests

Refer to `../codegen-client-test/README.md` for documentation on how to use this
module. This module is analogous to the `codegen-client-test` one, but it's named
`codegen-server-test` and it runs the `rust-server-codegen` Smithy build
plugin instead.

The [request limits integration tests](integration-tests/request-limits/tests/request_limits.rs)
check header deadlines, body size limits, default body-read deadlines, and
per-operation overrides against generated servers over TCP.

Each test starts its server in a Tokio task on an available loopback port and
aborts the task when it finishes. The tests check a 10s header deadline,
5s default / 3s operation override / 10s payload body deadlines, and a 1024-byte
body limit with both Content-Length and chunked framing. Generation settings
are defined in the `requestLimitsCodegenTest` entry in `build.gradle.kts`.

Run from the repository root:

```sh
./gradlew :codegen-server-test:assemble --quiet
cargo test --quiet --manifest-path codegen-server-test/integration-tests/Cargo.toml -p request-limits
```

After generation, only the Cargo command is needed to rerun these tests.
Regenerate the SDK after changing the Smithy model or generation settings.
The normal server integration-test task includes this package.
