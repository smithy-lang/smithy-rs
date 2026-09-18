---
applies_to: ["client", "aws-sdk-rust"]
authors: ["yychen23"]
references: ["smithy-rs#4805", "smithy-rs#4810", "smithy-rs#4827"]
breaking: true
new_feature: false
bug_fix: true
---
Across the Smithy runtime crates and generated SDK crates, `http` 0.2.x is now reached only through a named feature. Every crate below still supports it; nothing is removed.

**A default build is unchanged.** Generated SDK crates still enable the `rustls` feature by default, which builds the legacy `hyper` 0.14 / `rustls` 0.21 HTTP client and brings `http` 0.2.x with it. No crate is added to or removed from a default dependency tree by this change. Making the default client opt-in is a separate, later change.

What this change makes possible is *removing* `http` 0.2.x, which addresses the unpatched `http` 0.2.x advisories for builds that opt out. See "Removing `http` 0.2.x from your dependency tree" below.

### Recommended setup

Most users need no feature configuration and are unaffected:

```toml
aws-sdk-s3 = "..."
```

Enable `http-02x` (on the SDK crate, or on the individual runtime crate) only if you need the `http` 0.2.x interop APIs:

```toml
aws-sdk-s3 = { version = "...", features = ["http-02x"] }
```

### Removing `http` 0.2.x from your dependency tree

If you need `http` 0.2.x gone entirely — for example to satisfy a patch-compliance scan, since `http` 0.2.x has unpatched advisories — disable default features and re-enable the ones you need, omitting `rustls`:

```toml
aws-sdk-s3 = { version = "...", default-features = false, features = [
    "sigv4a", "http-1x", "default-https-client", "rt-tokio"
] }
```

That is `aws-sdk-s3`'s default feature list with `rustls` left out; the list varies slightly per SDK crate, so check the crate you depend on. The result has no `http` 0.2.x, `http-body` 0.4.x, `hyper` 0.14, `rustls` 0.21 or `h2` 0.3 in **either normal or dev scope**. `aws-config` needs no configuration — it already depends on the SDK crates it uses with `default-features = false` and defaults to `default-https-client`.

Cargo features are additive, so this is the only way to get a smaller tree: there is no feature that removes `http` 0.2.x, only features that add it.

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

**Bug fix:** `aws-smithy-runtime` now falls back to the `hyper` 1.x client when a `BehaviorVersion` older than `v2026_01_12` would otherwise get no default HTTP client at all. That path previously consulted only `connector-hyper-0-14-x`, so two configurations installed no client and failed every request with "No HTTP client was available to send this request": a build without the legacy connector, and a build with the connector but no TLS implementation, since the legacy `default_client` requires `legacy-rustls-ring`. Builds that do have a working legacy client are unaffected and continue to use it for those behavior versions.

This matters for the opt-out above. Leaving `rustls` out of the feature list removes the legacy connector, so without this fallback a client pinned to a `BehaviorVersion` older than `v2026_01_12` would come up with no HTTP client. Falling back is not silent: it logs a warning naming the feature to enable if you need the legacy stack.

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

- `test-util` no longer enables `aws-smithy-runtime`'s test features, so it no longer adds `http` 0.2.x on top of whatever the build already has. A build with `--features test-util` now resolves to the same HTTP stack as a build without it.
- A new opt-in **`legacy-test-util`** feature provides the pre-1.x test helpers for anyone who still needs them, and pulls `http` 0.2.x back in when enabled. It also enables `test-util`, so `--features legacy-test-util` on its own is enough to compile and run tests.

### `aws-runtime`

`aws-runtime`'s `http-02x` feature now enables `aws-smithy-types/http-body-0-4-x` itself. It previously inherited that feature transitively from `aws-smithy-runtime`, so enabling `aws-runtime/http-02x` on its own did not compile.
