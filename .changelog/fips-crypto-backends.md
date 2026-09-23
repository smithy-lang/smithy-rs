---
applies_to: ["client", "aws-sdk-rust"]
authors: ["yychen23"]
references: ["smithy-rs#4681"]
breaking: false
new_feature: true
bug_fix: false
---
`aws-smithy-checksums` can now compute its SHA-1 and SHA-256 checksums with [aws-lc-rs](https://github.com/aws/aws-lc-rs) instead of the RustCrypto hashers, including the FIPS 140-3 validated build of AWS-LC.

Nothing changes unless you ask for it: the new `rustcrypto` feature is on by default and keeps the existing behavior, byte for byte.

```toml
# non-FIPS AWS-LC
aws-smithy-checksums = { version = "...", features = ["aws-lc-rs"] }
# FIPS 140-3 validated AWS-LC
aws-smithy-checksums = { version = "...", features = ["fips"] }
```

Notes on the new features:

- `fips` takes precedence over `aws-lc-rs`, and either takes precedence over `rustcrypto`, so enabling more than one (which Cargo feature unification does routinely) resolves to the strongest backend rather than failing to build.
- FIPS is a per-target capability, not a universal one. `aws-lc-rs`'s `fips` feature builds `aws-lc-fips-sys`, which is only available on CMVP-validated operating environments — Linux, macOS, and Windows — and is unavailable on iOS and WASM (WASI and non-WASI). It also needs a C compiler, CMake, and Go at build time.
- MD5 is only available on the `rustcrypto` backend. aws-lc-rs does not expose MD5 and it is not FIPS-approved. This is not a behavior change: `ChecksumAlgorithm::Md5` is deprecated and already resolves to CRC-32, so no public API reaches MD5.
- These features cover checksums only. Routing SigV4 signing and TLS through a validated module is separate; see smithy-rs#4681 for the end-to-end picture.
