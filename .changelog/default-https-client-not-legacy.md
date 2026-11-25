---
applies_to: ["client", "aws-sdk-rust"]
authors: ["yychen23"]
references: ["smithy-rs#4805", "smithy-rs#4828"]
breaking: true
new_feature: false
bug_fix: false
---
Generated AWS SDK crates no longer enable the legacy `rustls` feature by default, so a default build no longer compiles the `hyper` 0.14 / `http` 0.2.x stack. Combined with the companion change making `http` 0.2.x opt-in across the runtime crates, a crate that depends on a generated SDK crate with default features has no `http` 0.2.x in its dependency tree, in either normal or dev scope. A default S3 client drops 12 packages, and only the 1.x generation resolves: `rustls` 0.23, `rustls-webpki` 0.103, `hyper` 1.x, `http` 1.x.

Several opt-in features still reach `http` 0.2.x: `legacy-https-client` (the `hyper` 0.14.x stack) and `legacy-test-util` (the pre-1.x test helpers) on every crate, and `http-02x` (the deprecated `PresignedRequest::*_http_02x_request` methods) on crates with presignable operations. Crates with integration tests also keep a dev-dependency on `aws-smithy-http-client` with `legacy-test-util`, so `http` 0.2.x stays visible when building the SDK workspace itself. Removing it is tracked in smithy-rs#5067.

This applies to the AWS SDK only. Clients smithy-rs generates for other Smithy services still enable `rustls` by default and still build the legacy stack.

The feature layout matches what `aws-config` already uses:

- `default-https-client` (the `hyper` 1.x client) remains a default feature.
- `rustls` is now a **synonym for `default-https-client`** and no longer a default feature. It was only ever a proxy for "give me an HTTPS client", so existing `features = ["rustls"]` call sites keep resolving and keep getting a working client, now the `hyper` 1.x one.
- The `hyper` 0.14.x + `rustls` 0.21.x stack is reachable only through the opt-in **`legacy-https-client`** feature, which already exists — it was added earlier as a second name for the same stack so call sites could pin it before `rustls` changed meaning. If you already use it, this change does not affect you.

**Breaking change:** if you use a `BehaviorVersion` older than `v2026_01_12` and rely on default features, your default HTTP client changes from the `hyper` 0.14 stack to `hyper` 1.x — a different TLS implementation, with different connection-pooling and timeout behavior. The same applies to `features = ["rustls"]`, which now selects the 1.x stack. This also affects you if you implement `HttpConnector`/`SharedHttpClient` against `http` 0.2.x types. To keep the legacy client:

```toml
aws-sdk-s3 = { version = "...", features = ["legacy-https-client"] }
```

**Breaking change:** with `default-features = false`, `features = ["rustls"]` now selects the `hyper` 1.x client on **every** behavior version, not just the old ones, since that spelling used to mean `tls-rustls` and enabled no `default-https-client`. It also builds `aws-lc-rs`/`aws-lc-sys` and its C build dependencies, where the old meaning resolved only `ring`; generated crates offer no `hyper` 1.x + `ring` alternative. Use `legacy-https-client` to keep the previous `ring`-based stack.

**Breaking change:** `aws-config`'s `legacy-client` feature is now a no-op. It supplied the legacy `hyper` 0.14.x client that behavior versions older than `v2026_01_12` selected; those now fall back to `hyper` 1.x, so enabling it no longer gets you that stack. It is still declared, so builds naming it keep resolving. To get the legacy stack, depend on `aws-smithy-runtime` directly:

```toml
aws-smithy-runtime = { version = "...", features = ["client", "tls-rustls"] }
```

`aws-smithy-runtime`'s `tls-rustls` no longer enables `aws-smithy-runtime-api/legacy-client`, which is likewise a deprecated no-op that gates no code. It stays declared, and still expands to `client` so that enabling `legacy-client` by itself keeps compiling. If you enable it directly, switch to `client`.

The advance-notice warning that announced this change is removed, since the change has now happened. A build that expected the legacy client but does not enable it gets the existing fallback warning instead, which reports the substitution rather than predicting it. That warning now fires the first time the substituted client is asked for a connector, once per process, so it no longer repeats for every client and operation and no longer appears for callers who supply their own `http_client`.
