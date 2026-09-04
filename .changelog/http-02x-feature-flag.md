---
applies_to: ["client", "aws-sdk-rust"]
authors: ["yychen23"]
references: ["smithy-rs#4805"]
breaking: true
new_feature: false
bug_fix: false
---
`aws-smithy-runtime-api` no longer depends on `http` 0.2.x by default. The crate's internal HTTP representations (`Headers`, `Uri`, `HttpError`, `EndpointPrefix`, and request/response extensions) now use the `http` 1.x types, and the `http` 0.2.x dependency has been made optional behind the pre-existing `http-02x` feature (off by default). This removes `http` 0.2.x from the default dependency tree of `aws-smithy-runtime-api`, addressing the unpatched `http` 0.2.x advisories. (Note: generated SDK clients still pull in `http` 0.2.x through other runtime crates; removing it from the full SDK tree is ongoing.)

**Breaking change:** the following previously-unconditional `pub` `http` 0.2.x conversion APIs are now only compiled when the `http-02x` feature is enabled. If you use any of them, enable the `http-02x` feature on `aws-smithy-runtime-api`:

- `Request::try_into_http02x` and `Response::try_into_http02x`
- `impl From<http_02x::Uri> for Uri`
- `impl TryInto<http_02x::Request<B>> for Request<B>` and `impl TryFrom<http_02x::Request<B>> for Request<B>`
- `impl TryFrom<http_02x::Response<B>> for Response<B>`
- `impl From<http_02x::StatusCode> for StatusCode` and `impl From<StatusCode> for http_02x::StatusCode`
- `impl TryFrom<http_02x::HeaderMap> for Headers`
- `impl AsHeaderComponent for http_02x::HeaderName` and `impl AsHeaderComponent for http_02x::HeaderValue`

Additionally, `Request::try_into_http02x` now returns an `Err` (instead of panicking) when the request URI is valid under `http` 1.x but not under `http` 0.2.x, and `TryFrom<http_02x::HeaderMap> for Headers` now returns an `Err` (instead of panicking) for header names that `http` 0.2.x accepts but `http` 1.x rejects.
