---
applies_to: ["client", "aws-sdk-rust"]
authors: ["yychen23"]
references: ["smithy-rs#4681"]
breaking: true
new_feature: true
bug_fix: false
---
`aws-sigv4` and `aws-smithy-checksums` can now perform their cryptography with [aws-lc-rs](https://github.com/aws/aws-lc-rs) instead of the RustCrypto crates, including the FIPS 140-3 validated build of AWS-LC. That covers SigV4 HMAC-SHA256 signing, SigV4a ECDSA-P256 signing, and SHA-1/SHA-256 request checksums.

A default build behaves exactly as before, byte for byte: RustCrypto is still the default backend, and it is still the only one that builds on every target the SDK supports. What is new is that selecting AWS-LC can now *remove* RustCrypto from the dependency graph rather than merely routing around it — the RustCrypto crates are optional dependencies behind a `rustcrypto` feature, so a build that disables default features and names an AWS-LC backend does not compile them at all. That is the point: a FIPS deployment that has to show which implementations are present can show their absence, not just their disuse.

The cost is that disabling default features now has consequences, on four crates. See the "Breaking changes" section at the end of this entry.

```toml
# non-FIPS AWS-LC
aws-sigv4 = { version = "...", features = ["aws-lc-rs"] }
aws-smithy-checksums = { version = "...", features = ["aws-lc-rs"] }
# FIPS 140-3 validated AWS-LC
aws-sigv4 = { version = "...", features = ["aws-lc-rs-fips"] }
aws-smithy-checksums = { version = "...", features = ["aws-lc-rs-fips"] }
```

Notes on the new features:

- `aws-lc-rs-fips` takes precedence over `aws-lc-rs`, and either takes precedence over `rustcrypto`, so enabling more than one (which Cargo feature unification does routinely) resolves to the strongest backend rather than failing to build.
- The `aws-lc-rs` floor is 1.18, matching the requirement `rustls` declares, so the signing, checksum and TLS paths all ask for the same AWS-LC line. Cargo resolves a single `aws-lc-rs` 1.x for the whole graph regardless — incompatible requirements force a downgrade elsewhere or fail outright rather than producing two copies — so this is not about avoiding duplicate AWS-LC builds. It keeps the declared requirements consistent, and it keeps CI's `minimal-versions --direct` leg, which pins direct dependencies to their declared minimum, exercising the line that builds actually ship.
- The floor is also an advisory floor rather than an API one; the APIs used here compile against 1.16.1. Because these crates offer both AWS-LC arms it has to be clean for both `-sys` crates: `aws-lc-sys` needs >= 0.39.0 (RUSTSEC-2026-0044, -0048) and `aws-lc-fips-sys` >= 0.13.13 (RUSTSEC-2026-0042, -0043), which the 0.14 line satisfies. Every `aws-lc-rs` before 1.17.3 — 1.17.0 included — declares `aws-lc-fips-sys = "^0.13.1"` and so permits an affected FIPS `-sys`.
- Both aws-lc-rs backends are a per-target capability, which is why `rustcrypto` stays the default — it is the only backend that builds everywhere the SDK does. `aws-lc-rs` needs only a C/C++ compiler and works on every target [aws-lc-rs supports](https://aws.github.io/aws-lc-rs/platform_support.html); the only WASM target it supports is `wasm32-unknown-emscripten`, so `wasm32-unknown-unknown` and the WASI targets have to stay on `rustcrypto`. `aws-lc-rs-fips` always additionally needs CMake and Go, plus bindgen on any target without pre-generated FIPS bindings — which is everything except 64-bit Linux (gnu and musl) and macOS, so Windows MSVC and FreeBSD FIPS builds need it. FIPS also covers a narrower target set than the non-FIPS arm: per that table it is x86_64 and aarch64 Linux (gnu and musl), `arm` musleabi/musleabihf, powerpc/powerpc64/powerpc64le gnu, macOS, 64-bit Windows MSVC, and x86_64 FreeBSD. Notably **not** `i686-unknown-linux-gnu`, riscv64, s390x, mips, NetBSD, iOS, Android, or WASM — the missing i686 support is why CI excludes these crates from its i686 `--all-features` leg.
- You usually don't need to set these per crate. Generated SDK crates and `aws-config` now carry a single `aws-lc-fips` feature that turns on all of it at once — see below.

`aws-sigv4` specifics:

- SigV4a signatures are non-deterministic, so switching the ECDSA implementation does not change any verifiable output. The signing key derivation is unchanged, and its 256-bit integer math stays on `crypto-bigint` in a FIPS build: that math is the key derivation the signing spec defines, not a cryptographic primitive, so it is not a gap in the FIPS story.
- With `sigv4a` and an aws-lc-rs backend both enabled, the `p256` crate is still compiled even though signing no longer calls it. Cargo features are additive, so `sigv4a` cannot declare `p256` only for the `rustcrypto` case. It carries no FIPS-relevant work in that configuration, and the test suite uses it to verify AWS-LC's signatures independently.

`aws-smithy-checksums` specifics:

- MD5 is only available on the RustCrypto backend. aws-lc-rs does not expose MD5 and it is not FIPS-approved. This is not a behavior change: `ChecksumAlgorithm::Md5` is deprecated and already resolves to CRC-32, so no public API reaches MD5.

## One switch for end-to-end FIPS

Generated SDK crates and `aws-config` have a new opt-in `aws-lc-fips` feature that routes **TLS, request signing, and request checksums** through the FIPS 140-3 validated build of AWS-LC together, instead of requiring you to align a feature on each runtime crate by hand:

```toml
aws-sdk-s3 = { version = "...", features = ["aws-lc-fips"] }
# or, for applications that configure through aws-config:
aws-config = { version = "...", features = ["aws-lc-fips"] }
```

That routes the cryptography through the validated module but leaves the RustCrypto crates compiled, because the `rustcrypto` default is still on. To keep them out of the build as well, turn the defaults off and name back the ones you want:

```toml
aws-sdk-dynamodb = { version = "...", default-features = false, features = [
    "aws-lc-fips",
    # The defaults you still want. Omit `rustcrypto`, which is the point of the exercise, and
    # omit `rustls`, which is the *legacy* TLS stack and would pull `ring` back in. Check your
    # crate's own `default` list -- a service crate with SigV4a or presigning defaults to more
    # than this one.
    "default-https-client", "rt-tokio",
] }
```

Measured on a generated `aws-sdk-dynamodb`, that shape resolves no `hmac`, `sha2`, `sha1`, `md-5`, or `ring`. Keeping the defaults and just adding `aws-lc-fips` resolves `hmac`, `sha2`, and `ring` — which is what the previous release did and why `default-features = false` is the part that matters.

Three things remain in such a build, none of them a non-validated implementation of a FIPS-relevant primitive:

- `aws-lc-sys`, the non-FIPS AWS-LC build, as soon as any TLS client is enabled. This is unchanged and is explained in the last bullet of the previous section: rustls stacks its `fips` feature on its `aws_lc_rs` feature, so the non-validated library is built but nothing references it and the linker drops it.
- `subtle`, pulled in by rustls. Constant-time comparison helpers, not a cryptographic primitive.
- With `sigv4a`, additionally `crypto-bigint`, `p256`, and — transitively through `p256` — `ecdsa`, `rfc6979`, and an older `hmac`/`sha2` pair (0.12/0.10, distinct from the 0.13/0.11 the backend would use). `crypto-bigint` does the integer math of the signing key derivation. `p256` is no longer called but cannot be dropped, because Cargo features are additive and `sigv4a` has to keep declaring it; the rest arrive as its dependencies. None of them is linked: a release build of an S3 client with `aws-lc-fips` and `sigv4a` contains zero `p256`, `ecdsa`, `rfc6979`, `hmac`, and `sha2` symbols, so this is build presence only.

Separately, a few generated crates declare a crypto crate directly, outside this mechanism and unaffected by these features: `aws-sdk-s3` (`hmac`, `sha2`, for the S3 Express session cache key), `aws-sdk-s3control` (`md-5`), and `aws-sdk-glacier` (`ring`).

The three paths reach a service crate by different routes, so enabling this feature fans out to `aws-smithy-runtime/aws-lc-fips` (TLS, via `aws-smithy-http-client`'s `rustls-aws-lc-fips`), `aws-runtime/aws-lc-fips` (signing, via `aws-sigv4/aws-lc-rs-fips`), and `aws-smithy-checksums/aws-lc-rs-fips`. The checksums arm is only present on service crates that have checksum operations, so a service without them doesn't gain the dependency. Two intermediate features are new and can also be used directly: `aws-runtime/aws-lc-fips` and `aws-smithy-runtime/aws-lc-fips`.

What this feature does not cover:

- TLS is only made FIPS for the hyper 1.x client from `aws-smithy-http-client`. Two ways a build can miss it:
  - **A `BehaviorVersion` older than `v2026_01_12`** selects the legacy hyper 0.14.x client, whose TLS is `rustls` 0.21 on `ring`. Signing and checksums are still FIPS, but TLS is not, so the build is not end-to-end FIPS. This combination now logs a warning naming the behavior version to move to; use `BehaviorVersion::v2026_01_12()` or later, or drop the legacy stack, to get FIPS TLS.
  - **An HTTP client installed explicitly** — `s2n-tls` or a custom connector — is unaffected, since this feature does not install a client for you. That case can't be detected and isn't warned about. If you use `s2n-tls`, note it reaches the same validated module by a different route — `s2n-tls/fips` forwards to `s2n-tls-sys/fips`, which is `aws-lc-rs/fips` — so enabling `s2n-tls`'s `fips` feature in your manifest does give you FIPS TLS; `aws-smithy-http-client` does not forward it for you.
- `aws-config`'s `credentials-login` feature signs DPoP (RFC 9449) proof JWTs with `p256` ECDSA itself, and that is not routed through AWS-LC. A FIPS deployment using `credentials-login` is not fully covered.
- `aws-config`'s `sso` and `credentials-login` features hash a start URL or session string with SHA-1 and SHA-256 to name a cache file. Those are RustCrypto and stay that way; they are not security functions.
- A build with `aws-lc-fips` compiles both AWS-LC libraries, because rustls's own `fips` feature stacks on its `aws_lc_rs` feature and so keeps `aws-lc-sys` in the graph. Only the validated one reaches the binary, though: `aws-lc-rs` routes every call to its FIPS build, so nothing references `aws-lc-sys` and the linker drops it. Measured on an unstripped release build of a minimal DynamoDB program with this feature plus the default HTTPS client: 2144 `aws_lc_fips_0_14_2_*` symbols and **zero** `aws_lc_0_45_0_*` symbols. So you pay the non-validated library's build time but do not ship it. The same binary also carries zero `hmac`, `sha2`, `sha1`, `md5`, and `ring` symbols, which is the binary-level counterpart to the dependency-graph result above. The absolute symbol count depends on the program; the zeroes are the point.

## Breaking changes

Making the RustCrypto crates optional means a build that names no backend has no crypto, and Cargo features are additive, so there is no way to express "RustCrypto unless AWS-LC is selected" without a `rustcrypto` feature that a FIPS build leaves off. The consequence is that `default-features = false` now drops the backend on four crates. Each affected build fails to compile with a message naming the fix, rather than silently losing cryptography:

```
error: aws-sigv4 requires a crypto backend: enable the `rustcrypto` (default), `aws-lc-rs`, or `aws-lc-rs-fips` feature.
```

`aws-sigv4` **1.6.0 → 2.0.0** and `aws-smithy-checksums` **0.65.0 → 0.66.0**: `--no-default-features` builds need a backend named.

```toml
# before
aws-sigv4 = { version = "1.6", default-features = false, features = ["sign-http", "http1"] }
# after
aws-sigv4 = { version = "2.0", default-features = false, features = ["sign-http", "http1", "rustcrypto"] }
```

`aws-runtime` **1.10.0 → 1.11.0**, a minor, with one behaviour change that would normally warrant more. This crate declares `aws-sigv4` with its defaults off now, which is how the choice reaches the signer, so it has a `default = ["rustcrypto"]` of its own. It previously had no `default` list at all, so `default-features = false` on it was a no-op; it now drops the crypto backend, and such a build fails with the `compile_error!` naming the fix. If you wrote that flag, add `rustcrypto`.

This ships under a minor deliberately. `aws_runtime::invocation_id`'s `SharedInvocationIdGenerator` and `InvocationIdGenerator` are allowed public surface of every generated SDK crate, and `aws-config` re-exports four `env_config` types. Under a major, an application naming `aws-runtime = "1"` and exchanging one of those types with its SDK client would stop compiling on a version-identity mismatch — two crate versions mean two distinct types — and staying consistent would require major-bumping every `aws-sdk-*` crate. A flag that previously did nothing is the narrower thing to break.

Generated SDK crates and `aws-config` gain a default-on `rustcrypto` feature for the same reason. Their `default` list already existed, so a build on defaults is unaffected; a build with `default-features = false` needs `rustcrypto` added unless it is naming `aws-lc-fips` on purpose.

Why the choice has to be forwarded at every level: Cargo offers a consumer no way to switch off a *transitive* crate's default features. Only the crate that declares a dependency can do that. So "no RustCrypto in this build" has to be expressible at each link in the chain, and the chain from an application to the signer is three deep — service crate, `aws-runtime`, `aws-sigv4`.
