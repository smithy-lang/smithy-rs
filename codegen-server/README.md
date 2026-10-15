# Smithy Rust Server Generator

Server-side Smithy code generator

[Design documentation](https://smithy-lang.github.io/smithy-rs/design/server/overview.html)

## Project Layout

* `codegen-server`: Server-side code generation.
* `codegen-server-test`: Server-side Smithy test and validation generation.
  Common commands:
  * `./gradlew :codegen-server-test:test` Generate code and run tests against it

## AWS JSON responses

AWS JSON 1.0 and 1.1 server responses ignore REST response bindings:
successful responses use HTTP 200, modeled client/server errors use HTTP 400/500,
and members annotated with `@httpHeader`, `@httpPrefixHeaders`, or
`@httpResponseCode` remain ordinary body members.

This behavior applies to both generated legacy serializers and
`codegen.schemaSerde: true`. There is no historical response-binding switch.

REST JSON/XML continue to apply REST response bindings. RPC v2 CBOR continues
to use HTTP 200 for success and honors `@httpError`, while retaining modeled
header members in the body only.
