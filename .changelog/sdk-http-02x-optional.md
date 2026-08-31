---
applies_to: ["aws-sdk-rust"]
authors: ["yychen23"]
references: ["smithy-rs#4805"]
breaking: true
new_feature: false
bug_fix: false
---
Generated SDK crates no longer depend on `http` 0.2.x by default. The `http` dependency is now optional and enabled by a new opt-in `http-02x` feature.

**Breaking change:** the deprecated `http` 0.2.x conversions on `PresignedRequest` are only available when the `http-02x` feature is enabled:

- `PresignedRequest::make_http_02x_request`
- `PresignedRequest::into_http_02x_request`

If you use either of them, enable the feature on the SDK crate, for example:

```toml
aws-sdk-s3 = { version = "...", features = ["http-02x"] }
```

Preferably, migrate to the `http` 1.x equivalents, which are enabled by default and are not deprecated: `PresignedRequest::make_http_1x_request` and `PresignedRequest::into_http_1x_request`.

Generated crates also no longer enable the `http-02x` feature on `aws-smithy-runtime-api` or `aws-runtime` unless the new `http-02x` feature is turned on.
