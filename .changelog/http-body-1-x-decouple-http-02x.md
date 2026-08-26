---
applies_to: ["client", "aws-sdk-rust"]
authors: ["yychen23"]
references: ["smithy-rs#4805"]
breaking: false
new_feature: false
bug_fix: false
---
Neither the `aws-smithy-types` `http-body-1-x` feature nor the `rt-tokio` feature pulls in the `http` 0.2.x or `http-body` 0.4.x crates anymore. `rt-tokio` now uses the `http-body` 1.x path for file-based bodies, and the legacy http-body 0.4.x adapter code (the `Http1toHttp04` body adapter, the 0.2.x header conversions, and the 0.4.x file-body impl) is gated behind the `http-body-0-4-x` feature. Combined with `aws-smithy-runtime-api` gating its `http` 0.2.x dependency behind `http-02x`, this removes `http` 0.2.x from the default dependency tree of the runtime crates — including for generated SDK clients, which enable both `rt-tokio` and `http-body-1-x` — addressing the unpatched `http` 0.2.x advisories. Consumers that need the http-body 0.4.x interop should enable the `http-body-0-4-x` feature.
