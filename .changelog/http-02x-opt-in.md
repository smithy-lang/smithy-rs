---
applies_to: ["client", "aws-sdk-rust"]
authors: ["yychen23"]
references: ["smithy-rs#4805", "smithy-rs#4810", "smithy-rs#4827"]
breaking: true
new_feature: false
bug_fix: false
---
`http` 0.2.x is now opt-in across the Smithy runtime crates and generated SDK crates, addressing the unpatched `http` 0.2.x advisories. Every crate below still supports `http` 0.2.x; it is simply no longer compiled unless you ask for it.

Together with the companion change that makes the legacy HTTP client opt-in, a default build of a generated AWS SDK crate no longer compiles `http` 0.2.x at all. Verified by compiling a crate that depends on `aws-sdk-s3` with a clean target directory: the `hyper` 1.x stack is built, and no `http` 0.2.x, `http-body` 0.4.x, `hyper` 0.14, `rustls` 0.21 or `h2` 0.3 crate is compiled.

Two qualifications on that claim:

- For a crate that depends on the SDK, `http` 0.2.x is absent from the dependency tree in **both normal and dev scope**. It is reachable only by explicitly enabling one of the opt-in features below.
- Clients that smithy-rs generates for other Smithy services still enable `rustls` by default, so they still build the legacy `hyper` 0.14 / `http` 0.2.x stack. The per-crate changes below apply to them; the default-client change does not.

### Recommended setup

Most users want the `hyper` 1.x client with no `http` 0.2.x. That is now the default, so no feature configuration is needed:

```toml
aws-sdk-s3 = "..."
```

Enable `http-02x` (on the SDK crate, or on the individual runtime crate) only if you still need the `http` 0.2.x interop APIs:

```toml
aws-sdk-s3 = { version = "...", features = ["http-02x"] }
```

### `aws-smithy-runtime-api`

`http` 0.2.x is now optional behind the pre-existing `http-02x` feature (off by default). The crate's internal HTTP representations (`Headers`, `Uri`, `HttpError`, `EndpointPrefix`, and request/response extensions) now use the `http` 1.x types.

**Breaking change:** these previously unconditional `pub` conversions now require the `http-02x` feature:

- `Request::try_into_http02x` and `Response::try_into_http02x`
- `impl From<http_02x::Uri> for Uri`
- `impl TryInto<http_02x::Request<B>> for Request<B>` and `impl TryFrom<http_02x::Request<B>> for Request<B>`
- `impl TryFrom<http_02x::Response<B>> for Response<B>`
- `impl From<http_02x::StatusCode> for StatusCode` and `impl From<StatusCode> for http_02x::StatusCode`
- `impl TryFrom<http_02x::HeaderMap> for Headers`
- `impl AsHeaderComponent for http_02x::HeaderName` and `impl AsHeaderComponent for http_02x::HeaderValue`

Additionally, `Request::try_into_http02x` now returns an `Err` instead of panicking when the request URI is valid under `http` 1.x but not under `http` 0.2.x, and `TryFrom<http_02x::HeaderMap> for Headers` now returns an `Err` instead of panicking for header names that `http` 0.2.x accepts but `http` 1.x rejects.

### `aws-smithy-types`

Neither the `http-body-1-x` feature nor the `rt-tokio` feature pulls in `http` 0.2.x or `http-body` 0.4.x anymore. `rt-tokio` now uses the `http-body` 1.x path for file-based bodies, and the legacy adapter code (the `Http1toHttp04` body adapter, the 0.2.x header conversions, and the 0.4.x file-body impl) is gated behind the `http-body-0-4-x` feature.

**Breaking change:** because `rt-tokio` no longer implies `http-body-0-4-x`, the `http` 0.2.x / `http-body` 0.4.x interop APIs are not available with only `rt-tokio` enabled. If you use `SdkBody::from_body_0_4`, `ByteStream::from_body_0_4`, or the `From<hyper_0_14::Body>` impls, enable `http-body-0-4-x`.

### `aws-smithy-runtime`

`http` 0.2.x and `http-body` 0.4.x are now optional behind a new `http-02x` feature (off by default), and the crate no longer forces on `aws-smithy-types`' `http-body-0-4-x` feature.

**Breaking change:** these `pub` modules now require the `http-02x` feature:

- `client::endpoint`, which contains the already-deprecated `apply_endpoint`. Its 1.8.0 deprecation notice already announced that it may be feature gated in a future minor version.
- `client::http::body::minimum_throughput::http_body_0_4_x`, which provides the `http_body::Body` 0.4.x implementations for `MinimumThroughputDownloadBody` and `ThroughputReadingBody`. Stalled stream protection is unaffected on the `http` 1.x path, which is what generated clients use.

**Breaking change:** the `test-util` feature no longer enables `legacy-test-util`, so it no longer pulls the `hyper` 0.14 / `http` 0.2.x ecosystem into the dependency tree. Two re-exports moved behind `legacy-test-util`, since both are the pre-1.x variants:

- `client::http::test_util::capture_request`
- `client::http::test_util::infallible_client_fn`

Keep them by enabling `legacy-test-util`, or migrate to the `http` 1.x equivalents in `aws_smithy_http_client::test_util`. `ReplayEvent`, `StaticReplayClient`, `NeverClient` and `capture_test_logs` are unaffected — they are already `http` 1.x or version-agnostic.

The legacy `connector-hyper-0-14-x` and `legacy-test-util` features otherwise work unchanged: they already pulled in the `http` 0.2.x ecosystem transitively through `aws-smithy-http-client`, and now declare what they need explicitly.

### `aws-sigv4`

The default-on `sign-http` feature no longer declares a dependency on `http` 0.2.x. Nothing compiled under that feature used it: request signing runs on `http` 1.x through `SigningInstructions::apply_to_request_http1x`, and the only `http` 0.2.x path, `apply_to_request_http0x`, is gated on `http0-compat`. `http` 0.2.x is now reachable only through `http0-compat`.

No public API changed. However, `aws-runtime` used to enable `aws-sigv4/http0-compat`, so applications depending on both crates were getting that feature switched on for them through Cargo feature unification. If you call `apply_to_request_http0x`, enable `aws-sigv4/http0-compat` explicitly.

### Generated SDK crates

The `http` dependency is now optional, enabled by a new opt-in `http-02x` feature. Generated crates also no longer enable `http-02x` on `aws-smithy-runtime-api` or `aws-runtime` unless that feature is turned on.

**Breaking change:** the deprecated `http` 0.2.x conversions on `PresignedRequest` require the `http-02x` feature:

- `PresignedRequest::make_http_02x_request`
- `PresignedRequest::into_http_02x_request`

Prefer migrating to the `http` 1.x equivalents, which are enabled by default and are not deprecated: `PresignedRequest::make_http_1x_request` and `PresignedRequest::into_http_1x_request`.

The generated test features changed too, so that building with `test-util` no longer drags `http` 0.2.x in:

- `test-util` no longer enables `aws-smithy-runtime`'s test features. A build with `--features test-util` now has no `http` 0.2.x in its dependency tree at all — it is identical to a default build.
- A new opt-in **`legacy-test-util`** feature provides the pre-1.x test helpers for anyone who still needs them, and pulls `http` 0.2.x back in when enabled. It also enables `test-util`, so `--features legacy-test-util` on its own is enough to compile and run tests.

### `aws-runtime`

`aws-runtime`'s `http-02x` feature now enables `aws-smithy-types/http-body-0-4-x` itself. It previously inherited that feature transitively from `aws-smithy-runtime`, so enabling `aws-runtime/http-02x` on its own did not compile.
