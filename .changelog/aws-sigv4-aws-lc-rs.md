---
applies_to: ["client", "aws-sdk-rust"]
authors: ["jplock"]
references: ["smithy-rs#4681"]
breaking: false
new_feature: true
bug_fix: false
---
Add optional `aws-lc-rs` and `fips` cargo features to `aws-sigv4`. When `aws-lc-rs` is enabled, the SigV4 HMAC-SHA256 / SHA-256 primitives — and, when combined with `sigv4a`, the SigV4a ECDSA-P256 signing path — are routed through `aws-lc-rs` instead of RustCrypto's `hmac` / `sha2` / `p256`. The `fips` feature additionally activates `aws-lc-rs/fips`, routing those primitives through `aws-lc-fips-sys`. The default build is unchanged.
