---
applies_to: ["client", "aws-sdk-rust"]
authors: ["jplock"]
references: ["smithy-rs#4681"]
breaking: false
new_feature: true
bug_fix: false
---
Add optional `aws-lc-rs` and `fips` cargo features to `aws-smithy-checksums`. When `aws-lc-rs` is enabled, SHA-1 and SHA-256 checksum computation is routed through `aws-lc-rs` instead of RustCrypto's `sha1` / `sha2`. The `fips` feature additionally activates `aws-lc-rs/fips`, routing those digests through `aws-lc-fips-sys`. The default build is unchanged.
