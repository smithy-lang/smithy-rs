---
applies_to: ["client", "aws-sdk-rust"]
authors: ["yychen23"]
references: ["smithy-rs#4827"]
breaking: false
new_feature: false
bug_fix: false
---
`aws-sigv4` no longer pulls in `http` 0.2.x as part of its default features. Its `sign-http` feature, which is enabled by default, declared `dep:http0` even though nothing compiled under that feature used it: request signing runs on `http` 1.x through `SigningInstructions::apply_to_request_http1x`, and the only `http` 0.2.x code path, `apply_to_request_http0x`, is gated on the `http0-compat` feature. `http` 0.2.x is now reachable only through `http0-compat`, and `aws-runtime` no longer enables that feature.

No public API changed. `apply_to_request_http0x` was already behind `http0-compat`, so enabling `sign-http` never exposed it.

One thing to be aware of if you call `apply_to_request_http0x`: because `aws-runtime` used to enable `aws-sigv4/http0-compat`, applications that depend on both crates were getting that feature switched on for them through Cargo feature unification. Those callers now need to enable `aws-sigv4/http0-compat` explicitly.

With this change, generated SDK clients built with `--no-default-features` have no `http` 0.2.x anywhere in their dependency tree. The default build still includes it through the legacy `rustls` feature, which selects the hyper 0.14 HTTP client stack.
