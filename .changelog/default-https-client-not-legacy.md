---
applies_to: ["client", "aws-sdk-rust"]
authors: ["yychen23"]
references: ["smithy-rs#4805", "smithy-rs#4828"]
breaking: true
new_feature: false
bug_fix: false
---
Generated AWS SDK crates no longer enable the legacy `rustls` feature by default, so a default build no longer compiles the `hyper` 0.14 / `http` 0.2.x stack. Combined with the companion change making `http` 0.2.x opt-in across the runtime crates, `http` 0.2.x is now absent from a generated SDK crate's default dependency tree in **both normal and dev scope**, reachable only through the opt-in features below. A default S3 client drops 12 packages.

This applies to the AWS SDK only. Clients smithy-rs generates for other Smithy services still enable `rustls` by default and still build the legacy stack.

The feature layout matches what `aws-config` already uses:

- `default-https-client` (the `hyper` 1.x client) remains a default feature.
- `rustls` is now a **synonym for `default-https-client`** and no longer a default feature. It was only ever a proxy for "give me an HTTPS client", so existing `features = ["rustls"]` call sites keep compiling and keep getting a working client.
- The `hyper` 0.14.x + `rustls` 0.21.x stack is reachable only through the opt-in **`legacy-https-client`** feature, which already exists — it was added earlier as a second name for the same stack so call sites could pin it before `rustls` changed meaning. If you already use it, this change does not affect you.

**Breaking change:** if you use a `BehaviorVersion` older than `v2026_01_12` and rely on default features, your default HTTP client changes from the `hyper` 0.14 stack to `hyper` 1.x — a different TLS implementation, with different connection-pooling and timeout behavior. The same applies to `features = ["rustls"]`, which now selects the 1.x stack. This also affects you if you implement `HttpConnector`/`SharedHttpClient` against `http` 0.2.x types. To keep the legacy client:

```toml
aws-sdk-s3 = { version = "...", features = ["legacy-https-client"] }
```

**Breaking change:** `aws-config`'s `legacy-client` feature is removed. It gated the pre-`v2026_01_12` behavior versions, which now fall back to the `hyper` 1.x client. For the `hyper` 0.14.x + `rustls` 0.21.x stack, depend on `aws-smithy-runtime` directly:

```toml
aws-smithy-runtime = { version = "...", features = ["client", "tls-rustls"] }
```

`aws-smithy-runtime`'s `tls-rustls` no longer enables `aws-smithy-runtime-api/legacy-client`. That feature is now a deprecated no-op: still declared, and still expanding to `client` so `tls-rustls` alone keeps working, but nothing reads it. It will be deleted once no released crate names it. If you enable it directly, switch to `client`.

The advance-notice warning that announced this change is removed, since the change has now happened. A build that expected the legacy client but does not enable it gets the existing fallback warning instead, which reports the substitution rather than predicting it. That warning now fires the first time the substituted client is asked for a connector, once per process, so it no longer repeats for every client and operation and no longer appears for callers who supply their own `http_client`.
