---
applies_to: ["client", "aws-sdk-rust"]
authors: ["yychen23"]
references: ["smithy-rs#4681"]
breaking: false
new_feature: true
bug_fix: false
---
`aws-sigv4` and `aws-smithy-checksums` can now perform their cryptography with [aws-lc-rs](https://github.com/aws/aws-lc-rs) instead of the RustCrypto crates, including the FIPS 140-3 validated build of AWS-LC. That covers SigV4 HMAC-SHA256 signing, SigV4a ECDSA-P256 signing, and SHA-1/SHA-256 request checksums.

Nothing changes unless you ask for it: the new `rustcrypto` feature is on by default on both crates and keeps the existing behavior, byte for byte.

```toml
# non-FIPS AWS-LC
aws-sigv4 = { version = "...", features = ["aws-lc-rs"] }
aws-smithy-checksums = { version = "...", features = ["aws-lc-rs"] }
# FIPS 140-3 validated AWS-LC
aws-sigv4 = { version = "...", features = ["fips"] }
aws-smithy-checksums = { version = "...", features = ["fips"] }
```

Notes on the new features:

- `fips` takes precedence over `aws-lc-rs`, and either takes precedence over `rustcrypto`, so enabling more than one (which Cargo feature unification does routinely) resolves to the strongest backend rather than failing to build.
- Both aws-lc-rs backends are a per-target capability, which is why `rustcrypto` stays the default — it is the only backend that builds everywhere the SDK does. `aws-lc-rs` needs a C/C++ compiler and works on every target [aws-lc-rs supports](https://aws.github.io/aws-lc-rs/platform_support.html); the only WASM target it supports is `wasm32-unknown-emscripten`, so `wasm32-unknown-unknown` and the WASI targets have to stay on `rustcrypto`. `fips` additionally needs CMake and Go, and covers a subset: Linux (gnu and musl), macOS, Windows MSVC, and FreeBSD — not iOS, not Android, not WASM.
- TLS is a separate axis, already available: `aws-smithy-http-client`'s `rustls-aws-lc-fips` feature. A single top-level switch that turns on all three at once is still to come; see smithy-rs#4681.

`aws-sigv4` specifics:

- SigV4a signatures are non-deterministic, so switching the ECDSA implementation does not change any verifiable output. The signing key derivation is unchanged, and its 256-bit integer math stays on `crypto-bigint` in a FIPS build: that math is the key derivation the signing spec defines, not a cryptographic primitive, so it is not a gap in the FIPS story.
- With `sigv4a` and an aws-lc-rs backend both enabled, the `p256` crate is still compiled even though signing no longer calls it. Cargo features are additive, so `sigv4a` cannot declare `p256` only for the `rustcrypto` case. It carries no FIPS-relevant work in that configuration, and the test suite uses it to verify AWS-LC's signatures independently.

`aws-smithy-checksums` specifics:

- MD5 is only available on the `rustcrypto` backend. aws-lc-rs does not expose MD5 and it is not FIPS-approved. This is not a behavior change: `ChecksumAlgorithm::Md5` is deprecated and already resolves to CRC-32, so no public API reaches MD5.
