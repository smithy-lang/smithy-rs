---
applies_to: ["client", "aws-sdk-rust"]
authors: ["yychen23"]
references: ["smithy-rs#4681"]
breaking: true
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

That routes the cryptography through the validated module but leaves the RustCrypto crates compiled, since the `rustcrypto` default is still on. To keep them out of the build as well, turn the defaults off and name back the ones you want:

```toml
aws-sdk-dynamodb = { version = "...", default-features = false, features = [
    "aws-lc-fips",
    # Omit `rustcrypto`, which is the point, and omit `rustls`, which is the *legacy* TLS stack
    # and would pull `ring` back in. Check your crate's own `default` list.
    "default-https-client", "rt-tokio",
] }
```

Two cases stay regardless, neither of which signs a request or computes a checksum: `sigv4a` declares `p256`, which brings `sha2` and `hmac` with it but is never called because the backend they serve isn't compiled, and `aws-sdk-s3` declares `hmac` and `sha2` to key an in-memory cache, which obfuscates rather than protects.

`aws-sigv4` and `aws-smithy-checksums` declare `aws-lc-rs = "1.18.1"`, on the same `1.18` line `rustls` declares so that signing, checksums and TLS all ask for the same AWS-LC. If your own manifest pins `aws-lc-rs` below that, raise it.

**Pinning the FIPS module.** `aws-lc-rs` moves to a new AWS-LC-FIPS module in a minor release, and Cargo takes that upgrade automatically inside `1.x`. We set no upper bound, so that choice is yours. If your compliance posture is tied to a particular module, check its validation status in [aws/aws-lc](https://github.com/aws/aws-lc/blob/main/crypto/fipsmodule/FIPS.md), work out which `aws-lc-rs` versions carry it, and bound your own requirement accordingly.

**[Full announcement, including platform support and the limits of the FIPS guarantee](https://github.com/awslabs/aws-sdk-rust/discussions/1464).** Read it before treating a build as end-to-end FIPS: TLS is only made FIPS for the hyper 1.x client, `aws-lc-rs-fips` covers a narrower set of targets than `aws-lc-rs` (notably not i686, WASM, iOS or Android), and `aws-config`'s `credentials-login` DPoP signing is not routed through AWS-LC.

## Breaking changes

Cargo features are additive, so "RustCrypto unless AWS-LC is selected" is not expressible; making RustCrypto a `rustcrypto` feature that a FIPS build leaves off is the only way to express its absence. The consequence is that `default-features = false` now drops the crypto backend on four crates. Each affected build fails to compile with a message naming the fix, rather than silently losing cryptography:

```
error: aws-sigv4 requires a crypto backend: enable the `rustcrypto` (default), `aws-lc-rs`, or `aws-lc-rs-fips` feature.
```

| crate | from | to | what to do |
|---|---|---|---|
| `aws-sigv4` | 1.6.6 | 1.7.0 | add `rustcrypto` to a `--no-default-features` build |
| `aws-smithy-checksums` | 0.65.1 | 0.65.2 | same |
| `aws-runtime` | 1.10.4 | 1.11.0 | same; this flag was previously a no-op here |
| `aws-config` | 1.13.3 | 1.14.0 | same |

Generated SDK crates gain a default-on `rustcrypto` feature for the same reason; their `default` list already existed, so only a `default-features = false` build is affected.

```toml
# before
aws-sigv4 = { version = "1.6", default-features = false, features = ["sign-http", "http1"] }
# after
aws-sigv4 = { version = "1.7", default-features = false, features = ["sign-http", "http1", "rustcrypto"] }
```

All of these ship below a major deliberately: a major on any one would split the released SDK's dependency graph, since `aws-runtime`'s invocation-id types are public surface of every generated SDK crate and `aws-sigv4`'s `SignableBody` is part of `aws-runtime`'s public API. The [announcement](https://github.com/awslabs/aws-sdk-rust/discussions/1464) has the reasoning.
