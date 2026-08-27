---
applies_to: ["client", "aws-sdk-rust"]
authors: ["yychen23"]
references: ["smithy-rs#4805"]
breaking: true
new_feature: false
bug_fix: false
---
Neither the `aws-smithy-types` `http-body-1-x` feature nor the `rt-tokio` feature pulls in the `http` 0.2.x or `http-body` 0.4.x crates anymore. `rt-tokio` now uses the `http-body` 1.x path for file-based bodies, and the legacy http-body 0.4.x adapter code (the `Http1toHttp04` body adapter, the 0.2.x header conversions, and the 0.4.x file-body impl) is gated behind the `http-body-0-4-x` feature. Combined with `aws-smithy-runtime-api` gating its `http` 0.2.x dependency behind `http-02x`, this reduces the `http` 0.2.x footprint of the runtime crates, addressing the unpatched `http` 0.2.x advisories. (Note: generated SDK clients still pull in `http` 0.2.x through other runtime crates such as `aws-smithy-runtime`; removing it from the full SDK tree is ongoing.)

**Breaking change:** because `rt-tokio` no longer implies the `http-body-0-4-x` feature, the `http` 0.2.x / `http-body` 0.4.x interop APIs are no longer available with only `rt-tokio` enabled. If you use `SdkBody::from_body_0_4`, `ByteStream::from_body_0_4`, or the `From<hyper_0_14::Body>` impls, enable the `http-body-0-4-x` feature on `aws-smithy-types`.
