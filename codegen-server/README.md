# Smithy Rust Server Generator

Server-side Smithy code generator

[Design documentation](https://smithy-lang.github.io/smithy-rs/design/server/overview.html)

## Project Layout

* `codegen-server`: Server-side code generation.
* `codegen-server-test`: Server-side Smithy test and validation generation.
  Common commands:
  * `./gradlew :codegen-server-test:test` Generate code and run tests against it

## AWS JSON response compatibility

AWS JSON 1.0 and 1.1 server responses ignore REST response bindings by default:
successful responses use HTTP 200, modeled client/server errors use HTTP 400/500,
and members annotated with `@httpHeader`, `@httpPrefixHeaders`, or
`@httpResponseCode` remain ordinary body members.

To preserve historical server responses, enable `legacyResponseBindings`
independently for each protocol in the `rust-server-codegen` plugin settings:

```json
{
  "customizationConfig": {
    "protocols": {
      "awsJson1_0": { "legacyResponseBindings": true },
      "awsJson1_1": { "legacyResponseBindings": true }
    }
  }
}
```

The flag defaults to `false`. When `true`, generated AWS JSON servers use the
operation's `@http` success status and the modeled error's `@httpError` status
(if present), and emit modeled response headers in addition to those members
in the body. `@httpResponseCode` members remain body data in either mode.

REST JSON/XML continue to apply REST response bindings. RPC v2 CBOR continues
to use HTTP 200 for success and honors `@httpError`, while retaining modeled
header members in the body only. These settings affect server generation only.
