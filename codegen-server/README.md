# Smithy Rust Server Generator

Server-side Smithy code generator

[Design documentation](https://smithy-lang.github.io/smithy-rs/design/server/overview.html)

## Serde terminology

* **Generated serde** (`codegen.schemaSerde: false`): codegen emits protocol serializers and deserializers.
* **Schema serde** (`codegen.schemaSerde: true`): the runtime reads shape schemas to serialize and deserialize.

HTTP version selection is separate from the serde mode. HTTP 0.x uses
`aws-smithy-legacy-http-server`; HTTP 1.x uses `aws-smithy-http-server` and supports both
serde modes. Reserve "legacy runtime" for the HTTP 0.x crate.

## Project Layout

* `codegen-server`: Server-side code generation.
* `codegen-server-test`: Server-side Smithy test and validation generation.
  Common commands:
  * `./gradlew :codegen-server-test:test` Generate code and run tests against it

## AWS JSON response bindings

AWS JSON 1.0 and 1.1 servers have always answered with the operation's `@http(code)`
success status, the modeled error's `@httpError` status (if present), and
`@httpHeader`/`@httpPrefixHeaders` members as response headers (each member also stays
in the document). `@httpResponseCode` members remain body data. This is the default,
with or without `codegen.schemaSerde`.

With `codegen.schemaSerde: true` you can instead follow the AWS JSON specification,
independently for each protocol in the `rust-server-codegen` plugin settings:

```json
{
  "codegen": { "schemaSerde": true },
  "customizationConfig": {
    "protocols": {
      "aws.protocols#awsJson1_0": { "responseBindings": "smithy-specs" },
      "aws.protocols#awsJson1_1": { "responseBindings": "historical" }
    }
  }
}
```

* `"historical"` (default): as described above.
* `"smithy-specs"`: status 200 for success, the `@error` class status (400/500) for errors, and
  every member in the document with no REST headers.

Servers generated without `schemaSerde` always behave as `"historical"` and leave
`customizationConfig.protocols` unparsed. REST JSON/XML apply REST response bindings and RPC v2 CBOR
ignores them, whatever this setting says. With `schemaSerde: true`, protocol settings must use full protocol shape IDs,
including custom protocols (for example, `com.example#customProtocol`). The shared `common` section is
the only exception. Shorthand keys such as `awsJson1_0` or `restJson1` are rejected during settings
loading. This validation does not apply to generated serde servers.
