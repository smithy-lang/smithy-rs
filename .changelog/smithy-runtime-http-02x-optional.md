---
applies_to: ["client", "aws-sdk-rust"]
authors: ["yychen23"]
references: ["smithy-rs#4827"]
breaking: true
new_feature: false
bug_fix: false
---
`aws-smithy-runtime` no longer depends on `http` 0.2.x or `http-body` 0.4.x by default. Both are now optional dependencies behind a new `http-02x` feature (off by default), and the crate no longer forces `aws-smithy-types`' `http-body-0-4-x` feature on. This removes `http` 0.2.x and `http-body` 0.4.x from the default dependency tree of `aws-smithy-runtime`, addressing the unpatched `http` 0.2.x advisories.

**Breaking change:** the following `pub` modules are now only compiled when the `http-02x` feature is enabled. If you use anything in them, enable the `http-02x` feature on `aws-smithy-runtime`:

- `client::endpoint` (which contains the already-deprecated `apply_endpoint`)
- `client::http::body::minimum_throughput::http_body_0_4_x`, which provides the `http_body::Body` 0.4.x implementations for `MinimumThroughputDownloadBody` and `ThroughputReadingBody`

The `apply_endpoint` deprecation notice added in 1.8.0 already announced that it may be feature gated in a future minor version. Stalled stream protection is unaffected on the `http` 1.x path, which is what generated clients use.

The legacy `connector-hyper-0-14-x` and `legacy-test-util` features continue to work unchanged: they already pulled in the `http` 0.2.x ecosystem transitively through `aws-smithy-http-client`, and now declare what they need explicitly.

Separately, `aws-runtime`'s `http-02x` feature now enables `aws-smithy-types/http-body-0-4-x` itself. It previously inherited that feature transitively from `aws-smithy-runtime`, so enabling `aws-runtime/http-02x` on its own did not compile.
