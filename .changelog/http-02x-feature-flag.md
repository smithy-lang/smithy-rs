---
applies_to: ["client", "aws-sdk-rust"]
authors: ["yychen23"]
references: ["smithy-rs#4805"]
breaking: false
new_feature: false
bug_fix: false
---
`aws-smithy-runtime-api` no longer depends on `http` 0.2.x by default. The crate's internal HTTP representations (`Headers`, `Uri`, `HttpError`, `EndpointPrefix`, and request/response extensions) now use the `http` 1.x types, and the `http` 0.2.x dependency has been made optional behind the pre-existing `http-02x` feature (off by default). Enable the `http-02x` feature to keep the `http` 0.2.x conversion APIs (for example `Request::try_into_http02x`, `From<http_02x::Uri>`, and `TryFrom<http_02x::HeaderMap>`). This addresses the unpatched `http` 0.2.x advisories by removing it from the default dependency tree of this crate.
