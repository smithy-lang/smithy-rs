---
applies_to: ["client", "aws-sdk-rust"]
authors: ["yychen23"]
references: ["smithy-rs#4681"]
breaking: false
new_feature: true
bug_fix: false
---
Request signing, request checksums, and TLS can now be routed through the FIPS 140-3 validated build of [AWS-LC](https://github.com/aws/aws-lc) with a single feature on your service crate. This covers SigV4 HMAC-SHA256 signing, SigV4a ECDSA-P256 signing and its SP 800-108 key derivation, and SHA-1/SHA-256 request checksums, which were previously computed by the RustCrypto crates.

**A default build signs and checksums exactly as before.** RustCrypto remains the default backend; its SigV4a signatures are pinned byte for byte against the path they replace, and both backends are held to the same known-answer vectors. It stays the default because the AWS-LC backends do not cover every target the SDK builds.

```toml
aws-sdk-s3 = { version = "...", features = ["aws-lc-fips"] }
# or, for applications that configure through aws-config:
aws-config = { version = "...", features = ["aws-lc-fips"] }
```

That routes the cryptography through the validated module but leaves the RustCrypto crates compiled. To keep them out of the build as well, set the `aws_sdk_fips_only` cfg:

```sh
RUSTFLAGS="--cfg aws_sdk_fips_only" cargo build
```

`hmac`, `sha2`, `sha1` and `md-5` are declared under `cfg(not(aws_sdk_fips_only))` in `aws-sigv4` and `aws-smithy-checksums`, so that flag drops them from the build and from `cargo tree -e normal`. (`aws-sigv4` also lists `sha2` as a dev-dependency for one test, so a plain `cargo tree` still shows it.) `RUSTFLAGS` is global, so one flag covers both crates; it is a cfg rather than a feature because features are additive, and "RustCrypto unless AWS-LC" cannot be expressed as one. On a service crate, also omit `rustls` when you turn default features off: that is the *legacy* TLS stack and pulls `ring` in. `aws-config`'s `rustls` is a synonym for `default-https-client` and is unaffected; its legacy stack is `legacy-client`.

Two cases stay regardless, neither of which signs a request or computes a checksum: `sigv4a` declares `p256` and `ecdsa`, which bring `sha2` and `hmac` with them but are never called because the backend they serve isn't compiled, and `aws-sdk-s3` declares `hmac` and `sha2` to key an in-memory cache, which obfuscates rather than protects.

`aws-sigv4` and `aws-smithy-checksums` declare `aws-lc-rs = "1.18.1"`, on the same `1.18` line `rustls` declares so that signing, checksums and TLS all ask for the same AWS-LC. If your own manifest pins `aws-lc-rs` below that, raise it.

**Which FIPS module you get.** The `1.18.1` floor builds against `aws-lc-fips-sys` 0.14, which is the AWS-LC-FIPS module 4 line. NIST validated module 4 under certificates [#5595](https://csrc.nist.gov/projects/cryptographic-module-validation-program/certificate/5595) (dynamic library) and [#5596](https://csrc.nist.gov/projects/cryptographic-module-validation-program/certificate/5596) (static library). [aws/aws-lc#3826](https://github.com/aws/aws-lc/pull/3826) is the documentation update for those and is still open, so [FIPS.md](https://github.com/aws/aws-lc/blob/main/crypto/fipsmodule/FIPS.md) lists module 4 as in process until it merges. Note also that an `aws-lc-fips-sys` release may vendor a later 4.x than a certificate names: 0.14.4 vendors AWS-LC 4.2.0. `aws-lc-rs` moves to a new module in a minor release and Cargo takes that automatically inside `1.x`; we set no upper bound, so bound your own requirement if your compliance posture is tied to one module.

**Limits of the guarantee.** TLS is only made FIPS for the hyper 1.x client, `aws-lc-rs-fips` covers a narrower set of targets than `aws-lc-rs` (notably not i686, WASM, iOS or Android), and `aws-config`'s `credentials-login` DPoP signing is not routed through AWS-LC. [Full announcement](https://github.com/awslabs/aws-sdk-rust/discussions/1464).

## Compatibility

Nothing breaks: `default-features = false` means what it always meant, and the new features are additive.

| crate | from | to | what is new |
|---|---|---|---|
| `aws-sigv4` | 1.6.6 | 1.7.0 | `aws-lc-rs`, `aws-lc-rs-fips` |
| `aws-smithy-checksums` | 0.65.1 | 0.65.2 | `aws-lc-rs`, `aws-lc-rs-fips` |
| `aws-runtime` | 1.10.4 | 1.11.0 | `aws-lc-fips` |
| `aws-config` | 1.13.4 | 1.14.0 | `aws-lc-fips` |
| `aws-smithy-runtime` | 1.16.4 | 1.17.0 | `aws-lc-fips` |

The one configuration that does not compile is `--cfg aws_sdk_fips_only` with no AWS-LC feature enabled, since that removes the RustCrypto backend without selecting a replacement. It fails with a message naming the fix, reported ahead of the unresolved imports that the same missing backend causes.
