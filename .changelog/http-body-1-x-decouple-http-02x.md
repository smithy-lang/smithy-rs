---
applies_to: ["client", "aws-sdk-rust"]
authors: ["yychen23"]
references: ["smithy-rs#4805"]
breaking: false
new_feature: false
bug_fix: false
---
The `aws-smithy-types` `http-body-1-x` feature no longer pulls in the `http` 0.2.x or `http-body` 0.4.x crates. The legacy http-body 0.4.x adapter code (the `Http1toHttp04` body adapter and 0.2.x header conversions) is now gated behind the `http-body-0-4-x` feature (and `rt-tokio`, which already enables the 0.4.x stack). Combined with `aws-smithy-runtime-api` gating its `http` 0.2.x dependency behind `http-02x`, this removes `http` 0.2.x from the default dependency tree of the runtime crates, addressing the unpatched `http` 0.2.x advisories. Consumers that need the http-body 0.4.x interop should enable the `http-body-0-4-x` feature.
