---
applies_to: ["client", "aws-sdk-rust"]
authors: ["yychen23"]
references: ["smithy-rs#4681"]
breaking: false
new_feature: true
bug_fix: false
---
Request signing, request checksums, and TLS can now be routed through the FIPS 140-3 validated build of [AWS-LC](https://github.com/aws/aws-lc-rs) with a single feature on your service crate. This covers SigV4 HMAC-SHA256 signing, SigV4a ECDSA-P256 signing, and SHA-1/SHA-256 request checksums, which were previously computed by the RustCrypto crates.

**A default build is unchanged, byte for byte.** RustCrypto remains the default backend and is still the only one that builds on every target the SDK supports.

```toml
aws-sdk-s3 = { version = "...", features = ["aws-lc-fips"] }
# or, for applications that configure through aws-config:
aws-config = { version = "...", features = ["aws-lc-fips"] }
```

That routes the cryptography through the validated module but leaves the RustCrypto crates compiled. To keep them out of the build as well, set the `aws_sdk_fips_only` cfg:

```sh
RUSTFLAGS="--cfg aws_sdk_fips_only" cargo build
```

`hmac`, `sha2`, `sha1` and `md-5` are declared under `cfg(not(aws_sdk_fips_only))` in `aws-sigv4` and `aws-smithy-checksums`, so that flag drops them from the build and from `cargo tree`. `RUSTFLAGS` is global, so one flag covers both crates. Note it is a cfg rather than a Cargo feature because features are additive: "RustCrypto unless AWS-LC" cannot be expressed as one, and making these optional would break every build that lists the default features explicitly. Also omit `rustls` if you turn default features off, since that is the *legacy* TLS stack and would pull `ring` back in.

Two cases stay regardless, neither of which signs a request or computes a checksum: `sigv4a` declares `p256`, which brings `sha2` and `hmac` with it but is never called because the backend they serve isn't compiled, and `aws-sdk-s3` declares `hmac` and `sha2` to key an in-memory cache, which obfuscates rather than protects.

`aws-sigv4` and `aws-smithy-checksums` declare `aws-lc-rs = "1.18.1"`, on the same `1.18` line `rustls` declares so that signing, checksums and TLS all ask for the same AWS-LC. If your own manifest pins `aws-lc-rs` below that, raise it.

**Pinning the FIPS module.** `aws-lc-rs` moves to a new AWS-LC-FIPS module in a minor release, and Cargo takes that upgrade automatically inside `1.x`. We set no upper bound, so that choice is yours. If your compliance posture is tied to a particular module, check its validation status in [aws/aws-lc](https://github.com/aws/aws-lc/blob/main/crypto/fipsmodule/FIPS.md), work out which `aws-lc-rs` versions carry it, and bound your own requirement accordingly.

**[Full announcement, including platform support and the limits of the FIPS guarantee](https://github.com/awslabs/aws-sdk-rust/discussions/1464).** Read it before treating a build as end-to-end FIPS: TLS is only made FIPS for the hyper 1.x client, `aws-lc-rs-fips` covers a narrower set of targets than `aws-lc-rs` (notably not i686, WASM, iOS or Android), and `aws-config`'s `credentials-login` DPoP signing is not routed through AWS-LC.

## Compatibility

Nothing breaks. `default-features = false` means what it always meant on every crate involved, and a build that names no AWS-LC feature gets the same RustCrypto implementations as before. The new features are additive:

| crate | from | to | what is new |
|---|---|---|---|
| `aws-sigv4` | 1.6.6 | 1.7.0 | `aws-lc-rs`, `aws-lc-rs-fips` |
| `aws-smithy-checksums` | 0.65.1 | 0.65.2 | `aws-lc-rs`, `aws-lc-rs-fips` |
| `aws-runtime` | 1.10.4 | 1.11.0 | `aws-lc-fips` |
| `aws-config` | 1.13.3 | 1.14.0 | `aws-lc-fips` |
| `aws-smithy-runtime` | 1.16.4 | 1.17.0 | `aws-lc-fips` |

The one configuration that does not compile is `--cfg aws_sdk_fips_only` with no AWS-LC feature enabled, since that removes the RustCrypto backend without selecting a replacement. It fails with a message naming the fix rather than on an unresolved import.
