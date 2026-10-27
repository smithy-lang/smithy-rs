# Smithy Rust Server Generator

Server-side Smithy code generator

[Design documentation](https://smithy-lang.github.io/smithy-rs/design/server/overview.html)

## Serde terminology

* **Generated serde** (`codegen.schemaSerde: false`): codegen emits protocol serializers and deserializers.
* **Schema serde** (`codegen.schemaSerde: true`): the runtime reads shape schemas to serialize and deserialize.

HTTP version selection is separate from the serde mode. HTTP 0.x uses
`aws-smithy-legacy-http-server`; HTTP 1.x uses `aws-smithy-http-server` and supports both
serde modes. Reserve "legacy runtime" for the HTTP 0.x crate.

## Request-read configuration

Set request-read settings in the `rust-server-codegen` plugin configuration in
`smithy-build-template.json`, under `customizationConfig.protocols.common.request`.
Request-read settings use the shared `common` section in either serde mode.
Schema protocol sections use full Smithy protocol shape IDs. For example:

```json
{
  "codegen": {
    "http-1x": true
  },
  "customizationConfig": {
    "protocols": {
      "common": {
        "request": {
          "header": {
            "readTimeout": "10s"
          },
          "body": {
            "maxBytes": "100 MB",
            "readPollBudget": 16,
            "readTimeouts": {
              "defaultNonPayload": "2m",
              "defaultPayload": "30m",
              "perOperation": {
                "com.example#Upload": "10m"
              }
            },
            "eventStreamTimeouts": {
              "initialMessage": "10s",
              "messageCompletion": "30s",
              "idle": "5m",
              "perOperation": {
                "com.example#PublishEvents": {
                  "idle": 0
                }
              }
            }
          }
        }
      }
    }
  }
}
```

Use full operation shape IDs from your model. Regenerate the SDK after changing
the JSON configuration. Body settings are fixed for each generated SDK.
Body timeout overrides replace the applicable default;
event-stream timeout overrides inherit fields omitted from their JSON object.
Whole-body deadlines do not apply to streaming inputs. The read poll budget
currently controls consecutive ready event-stream HTTP body-frame reads.

`maxBytes` accepts integer byte counts or quoted whole-number sizes:
`"100 KB"` is 100,000 bytes, `"100 MB"` is 100,000,000 bytes, and `"100 MiB"`
is 104,857,600 bytes. Supported units are `B`, `KB`, `MB`, `GB`, `TB`, `KiB`,
`MiB`, `GiB`, and `TiB`; units are case-sensitive. Whitespace is optional,
so `"100MB"` also works. `0` or `"0 MB"` removes the size limit. Negative
values, fractions, unknown units, and counts exceeding `Long.MAX_VALUE` fail
generation. Generated Rust uses `Option<NonZeroUsize>`, so the count must also
fit the Rust target's `usize`. Size strings are parsed by Kotlin during
generation; the runtime receives byte counts.

`codegen.requestBodyMaxBytes` is deprecated and emits a generation warning
pointing to `customizationConfig.protocols.common.request.body.maxBytes`.
The old key remains supported when the nested setting is absent. If both are
present, the nested value takes precedence.

## Event-stream deadlines

The three `request.body.eventStreamTimeouts` settings cover separate read phases:

| Setting | RPC event streams | REST event streams |
| --- | --- | --- |
| `initialMessage` | Bounds reading the entire first frame before invoking the handler, including waiting for its first bytes. | Does not apply. |
| `messageCompletion` | Bounds each frame after the first, from its first bytes until the complete frame is read. | Bounds every frame from its first bytes until the complete frame is read. |
| `idle` | Bounds waiting for the next frame's first bytes after the first frame completes. | Bounds waiting for the first frame's bytes and for each subsequent frame's first bytes. |

The first RPC frame can be an `initial-request` message carrying non-stream
input fields or an ordinary event for an operation with only an event-stream
input. Both are read before invoking the handler and use only `initialMessage`.
Its deadline starts before waiting for bytes and does not reset as bytes arrive.

If `initialMessage` is disabled, the first RPC frame has no event-stream read
deadline, even when `messageCompletion` or `idle` is enabled. Neither setting
provides a fallback for this phase. Configure a nonzero `initialMessage` for RPC
operations to bound senders that withhold bytes or send an incomplete first
frame. Request-header deadlines end before body reads, and whole-body deadlines
do not apply to streaming inputs.

All three event-stream deadlines are disabled by default. Numeric `0` or a
zero-duration string disables the corresponding deadline; omitted operation
fields inherit the service setting. After the first RPC frame, or for any REST
frame, completion and idle timeouts surface as resumable errors from the
handler's `recv()` call. An initial-message timeout rejects the request with
HTTP `408` before the handler runs.

## Request-header deadline

With `codegen.http-1x: true`, the generated
`sdk::serve(listener, app.into_make_service())` automatically applies this timeout.
Omitting it keeps the one-minute runtime default. Numeric `0` or a zero-duration
string such as `"0s"` disables it. Durations use whole numbers with `ms`, `s`, `m`,
or `h` units; invalid or overflowing values fail generation.

An explicit `.header_read_timeout(Some(duration))` call on the returned `Serve`
builder overrides the generated setting. `.header_read_timeout(None)` or
`.header_read_timeout(Some(Duration::ZERO))` disables it. The function's arguments
and return type remain the same as the runtime's `serve`.

The first-request deadline starts after connection service creation, includes
protocol negotiation and header parsing, and does not reset as bytes arrive.
It covers the first HTTP/1 or HTTP/2 request head and subsequent HTTP/1 heads.
Later HTTP/2 streams have no header deadline; keep-alive pings do not provide one.
It does not limit middleware, body reads, handler execution, or waiting for server
events. The Tokio time driver must be enabled.

Both runtimes carry the setting in the same typed service configuration.
The shared configuration types live in `aws_smithy_http_server::config` and
`aws_smithy_legacy_http_server::config`, including `ServiceConfig`, request-body
allowances, and event-stream deadlines.
Legacy servers (`codegen.http-1x: false`) manage their own Hyper accept loop and
must apply header timeouts themselves; there is no generated `serve` function.
Calling the current runtime's `aws_smithy_http_server::serve` directly also uses
the runtime's own defaults.

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
ignores them, whatever this setting says. With `schemaSerde: true`, protocol settings must use full protocol shape IDs,
including custom protocols (for example, `com.example#customProtocol`). The shared `common` section is
the only exception. Shorthand keys such as `awsJson1_0` or `restJson1` are rejected during settings
loading. This validation does not apply to generated serde servers.

AWS JSON routing uses the modeled operation name: `getFoo` routes with
`X-Amz-Target: Example.getFoo`. Set `"capitalizeRoutes": true` in the
`aws.protocols#awsJson1_0` or `aws.protocols#awsJson1_1` protocol section to also
accept `Example.GetFoo`. Both spellings coexist when enabled; the default is
`false`. This option is honored in both serde modes. Generated serde reads this
routing option without applying other schema-only protocol settings.
