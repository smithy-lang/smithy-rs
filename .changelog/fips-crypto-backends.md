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
- You usually don't need to set these per crate. Generated SDK crates and `aws-config` now carry a single `fips` feature that turns on all of it at once — see below.

`aws-sigv4` specifics:

- SigV4a signatures are non-deterministic, so switching the ECDSA implementation does not change any verifiable output. The signing key derivation is unchanged, and its 256-bit integer math stays on `crypto-bigint` in a FIPS build: that math is the key derivation the signing spec defines, not a cryptographic primitive, so it is not a gap in the FIPS story.
- With `sigv4a` and an aws-lc-rs backend both enabled, the `p256` crate is still compiled even though signing no longer calls it. Cargo features are additive, so `sigv4a` cannot declare `p256` only for the `rustcrypto` case. It carries no FIPS-relevant work in that configuration, and the test suite uses it to verify AWS-LC's signatures independently.

`aws-smithy-checksums` specifics:

- MD5 is only available on the `rustcrypto` backend. aws-lc-rs does not expose MD5 and it is not FIPS-approved. This is not a behavior change: `ChecksumAlgorithm::Md5` is deprecated and already resolves to CRC-32, so no public API reaches MD5.

## One switch for end-to-end FIPS

Generated SDK crates and `aws-config` have a new opt-in `fips` feature that routes **TLS, request signing, and request checksums** through the FIPS 140-3 validated build of AWS-LC together, instead of requiring you to align a feature on each runtime crate by hand:

```toml
aws-sdk-s3 = { version = "...", features = ["fips"] }
# or, for applications that configure through aws-config:
aws-config = { version = "...", features = ["fips"] }
```

The three paths reach a service crate by different routes, so enabling this feature fans out to `aws-smithy-runtime/crypto-fips` (TLS, via `aws-smithy-http-client`'s `rustls-aws-lc-fips`), `aws-runtime/fips` (signing, via `aws-sigv4/fips`), and `aws-smithy-checksums/fips`. The checksums arm is only present on service crates that have checksum operations, so a service without them doesn't gain the dependency. Two intermediate features are new and can also be used directly: `aws-runtime/fips` and `aws-smithy-runtime/crypto-fips`.

What this feature does not cover:

- TLS is only made FIPS for the `aws-smithy-http-client`-based client. A build whose TLS comes from elsewhere — `s2n-tls`, the legacy ring-backed `tls-rustls` feature, or a custom connector — is unaffected, and its TLS is not FIPS. The feature does not install an HTTP client for you, so this is silent; check your client configuration.
- `aws-config`'s `credentials-login` feature signs DPoP (RFC 9449) proof JWTs with `p256` ECDSA itself, and that is not routed through AWS-LC. A FIPS deployment using `credentials-login` is not fully covered.
- `aws-config`'s `sso` and `credentials-login` features hash a start URL or session string with SHA-1 and SHA-256 to name a cache file. Those are RustCrypto and stay that way; they are not security functions.
- A build with `fips` also contains the non-validated `aws-lc-sys`, because rustls's own `fips` feature stacks on its `aws_lc_rs` feature. Both AWS-LC builds are compiled; aws-lc-rs uses the validated one, so the crypto in use is the validated module.
