# Smithy Rust Server Generator

Server-side Smithy code generator

[Design documentation](https://smithy-lang.github.io/smithy-rs/design/server/overview.html)

## Serde terminology

* **Generated serde** (`codegen.schemaSerde: false`): codegen emits protocol serializers and deserializers.
* **Schema serde** (`codegen.schemaSerde: true`): the runtime reads shape schemas to serialize and deserialize.

HTTP version selection is separate from the serde mode. HTTP 0.x uses
`aws-smithy-legacy-http-server`; HTTP 1.x uses `aws-smithy-http-server` and supports both
serde modes. Reserve "legacy runtime" for the HTTP 0.x crate.

Generated servers use Tower 0.5 with either serde mode. HTTP 1.x servers can use
`aws-smithy-http-server-metrics` with generated serde or schema serde.

## Shared runtime configuration

Schema service builders emit typed request-body settings through
`aws_smithy_http_server::config::{ServiceConfig, ServiceRequestBodyConfig,
RequestBodyCollectionConfig}`. Routing and serialization remain under `schema`;
configuration is exposed only through `config`.

Both serde modes read the buffered request-body size limit from
`customizationConfig.protocols.common.request.body.maxBytes`:

```json
{
  "customizationConfig": {
    "protocols": {
      "common": {
        "request": {
          "body": { "maxBytes": "100 MB" }
        }
      }
    }
  }
}
```

`maxBytes` accepts non-negative integer byte counts or whole-number size strings
using `B`, `KB`, `MB`, `GB`, `TB`, `KiB`, `MiB`, `GiB`, or `TiB`. An absent setting
inherits the runtime default; `0` explicitly disables the limit.
`codegen.requestBodyMaxBytes` is deprecated and emits a generation warning pointing
to the nested setting. It remains a fallback when the nested setting is absent;
when both are present, the nested value wins and codegen warns about the conflict.
The limit applies to buffered bodies, including non-streaming payloads.

Schema serde reads its frame poll budget from the same body section's
`readPollBudget` key; it defaults to 32, and `0` disables forced yielding.
Generated serde retains its existing polling behavior.

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

Servers generated without `schemaSerde` always behave as `"historical"` for response bindings.
REST JSON/XML apply REST response bindings and RPC v2 CBOR
ignores them, whatever this setting says. In both serde modes, protocol settings must use full protocol shape IDs,
including custom protocols (for example, `example.protocols#customRpc`). The shared `common` section is
the only exception. Shorthand keys such as `awsJson1_0` or `restJson1` are rejected during settings
loading. Unknown common request-setting keys emit a warning.

AWS JSON routing uses the modeled operation name: `getFoo` routes with
`X-Amz-Target: Example.getFoo`. Set `"capitalizeRoutes": true` in the
`aws.protocols#awsJson1_0` or `aws.protocols#awsJson1_1` protocol section to also
accept `Example.GetFoo`. Both spellings coexist when enabled; the default is
`false`. This option is honored in both serde modes. Generated serde reads this
routing option without applying other schema-only protocol settings.
