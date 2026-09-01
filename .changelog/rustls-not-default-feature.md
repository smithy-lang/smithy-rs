---
applies_to: ["client", "aws-sdk-rust"]
authors: ["yychen23"]
references: ["smithy-rs#4805"]
breaking: true
new_feature: false
bug_fix: false
---
Generated SDK clients no longer enable the legacy `rustls` feature by default, so `http` 0.2.x is no longer built as part of a default client. Together with the preceding changes to `aws-smithy-runtime-api`, `aws-smithy-types`, `aws-smithy-runtime` and `aws-sigv4`, a default client build now contains **no `http` 0.2.x anywhere in its dependency tree**, addressing the unpatched `http` 0.2.x advisories.

Previously a default build compiled two complete generations of the HTTP/TLS stack side by side — `hyper` 0.14 and 1.x, `http` 0.2.x and 1.x, `h2` 0.3 and 0.4, and `rustls` 0.21 and 0.23, each with its own TLS implementation. Only the 1.x stack is built now, which removes 13 crates from a default S3 client.

The feature layout matches what `aws-config` already adopted:

- `default-https-client` (the `hyper` 1.x client) remains a default feature.
- `rustls` is now a **synonym for `default-https-client`** and is no longer a default feature. It was never more than a proxy for "give me an HTTPS client", so existing `features = ["rustls"]` call sites keep compiling and keep getting a working client.
- The `hyper` 0.14.x + `rustls` 0.21.x stack moved to a new opt-in **`legacy-client`** feature.

**Breaking change:** if you use `BehaviorVersion` older than `v2026_01_12` and rely on default features, your default HTTP client changes from the `hyper` 0.14 stack to the `hyper` 1.x stack. That means a different TLS implementation and different connection-pooling and timeout behavior. The same applies if you enable `features = ["rustls"]`, which now selects the 1.x stack. To keep the legacy client, enable `legacy-client`:

```toml
aws-sdk-s3 = { version = "...", features = ["legacy-client"] }
```

This also applies if you implement `HttpConnector`/`SharedHttpClient` against `http` 0.2.x types, or otherwise depend on the legacy stack being present in the tree.

Separately, `aws-smithy-runtime` now falls back to the `hyper` 1.x client when a `BehaviorVersion` older than `v2026_01_12` would otherwise get no default HTTP client at all. Previously that path consulted only `connector-hyper-0-14-x`, so a build without the legacy connector — or with the connector but no TLS implementation, since `hyper_014::default_client` requires `legacy-rustls-ring` — installed no client and failed every request with "No HTTP client was available to send this request". Builds that do have a working legacy client are unaffected and continue to use it for those behavior versions.
