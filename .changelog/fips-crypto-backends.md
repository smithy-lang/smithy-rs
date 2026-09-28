---
applies_to: ["client", "aws-sdk-rust"]
authors: ["yychen23"]
references: ["smithy-rs#4681"]
breaking: false
new_feature: true
bug_fix: false
---
`aws-sigv4` and `aws-smithy-checksums` can now perform their cryptography with [aws-lc-rs](https://github.com/aws/aws-lc-rs) instead of the RustCrypto crates, including the FIPS 140-3 validated build of AWS-LC. That covers SigV4 HMAC-SHA256 signing, SigV4a ECDSA-P256 signing, and SHA-1/SHA-256 request checksums.

Nothing changes unless you ask for it. The RustCrypto crates stay unconditional dependencies and remain the backend unless you select an AWS-LC one, so a default build behaves exactly as before, byte for byte. Selecting AWS-LC changes which implementation runs; it does not remove RustCrypto from the dependency tree.

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
- The `aws-lc-rs` floor is 1.17.3, chosen to exclude `-sys` versions with open advisories rather than for its API — the APIs used here compile against 1.16.1. Because these crates offer both AWS-LC arms, the floor has to be clean for both `-sys` crates, and 1.17.3 is the lowest version that is: `aws-lc-sys` needs >= 0.39.0 (RUSTSEC-2026-0044, -0048) and `aws-lc-fips-sys` needs >= 0.13.13 (RUSTSEC-2026-0042, -0043). 1.16.2 already reaches a clean `aws-lc-sys` (^0.39.0), but every version before 1.17.3 — 1.17.0 included — declares `aws-lc-fips-sys = "^0.13.1"` and so permits an affected FIPS `-sys`. Cargo normally resolves to the highest version, so this is a latent rather than active exposure, but the floor is what guards it.
- Which FIPS module you get follows `aws-lc-rs`, and the choice is not simply "newer is worse". 1.17.x resolves `aws-lc-fips-sys` 0.13.x, FIPS module 3, which held CMVP certificates #5314 (static) and #5298 (dynamic) as of September 2026 but sits on a non-LTS branch, which [AWS-LC's versioning policy](https://github.com/aws/aws-lc/blob/main/VERSIONING.md) says consumers should not depend on and which carries no stated support window. 1.18.x resolves 0.14.x, FIPS module 4, which is on the CMVP Modules In Process list but sits on the LTS branch, with a five-year support commitment. Both lines are in fact still being maintained — `aws-lc-fips-sys` 0.13.17 and 0.14.2 were published the same day — so this is a difference in commitment rather than in current patch availability. Cargo resolves to the highest compatible version, so a fresh lockfile takes module 4; for most deployments that is the right default.
- If you nevertheless need module 3, request it as `aws-lc-rs = "~1.17"` and be aware of the cost. Do **not** write `aws-lc-rs = "<1.18.0"`: it has no lower bound, so Cargo can satisfy it with a placeholder version that has no lib target, leaving your real dependency on module 4 with only a warning. Also note `rustls` 0.23.44 and later require `aws-lc-rs = "1.18"`, so a `~1.17` request resolves by downgrading `rustls` to 0.23.43 — silently, since it is a successful resolution rather than a conflict. A module-3 build today means an older TLS stack.
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

The three paths reach a service crate by different routes, so enabling this feature fans out to `aws-smithy-runtime/aws-lc-fips` (TLS, via `aws-smithy-http-client`'s `rustls-aws-lc-fips`), `aws-runtime/aws-lc-fips` (signing, via `aws-sigv4/aws-lc-rs-fips`), and `aws-smithy-checksums/aws-lc-rs-fips`. The checksums arm is only present on service crates that have checksum operations, so a service without them doesn't gain the dependency. Two intermediate features are new and can also be used directly: `aws-runtime/aws-lc-fips` and `aws-smithy-runtime/aws-lc-fips`.

What this feature does not cover:

- TLS is only made FIPS for the hyper 1.x client from `aws-smithy-http-client`. Two ways a build can miss it:
  - **A `BehaviorVersion` older than `v2026_01_12`** selects the legacy hyper 0.14.x client, whose TLS is `rustls` 0.21 on `ring`. Signing and checksums are still FIPS, but TLS is not, so the build is not end-to-end FIPS. This combination now logs a warning naming the behavior version to move to; use `BehaviorVersion::v2026_01_12()` or later, or drop the legacy stack, to get FIPS TLS.
  - **An HTTP client installed explicitly** — `s2n-tls` or a custom connector — is unaffected, since this feature does not install a client for you. That case can't be detected and isn't warned about. If you use `s2n-tls`, note it reaches the same validated module by a different route — `s2n-tls/fips` forwards to `s2n-tls-sys/fips`, which is `aws-lc-rs/fips` — so enabling `s2n-tls`'s `fips` feature in your manifest does give you FIPS TLS; `aws-smithy-http-client` does not forward it for you.
- `aws-config`'s `credentials-login` feature signs DPoP (RFC 9449) proof JWTs with `p256` ECDSA itself, and that is not routed through AWS-LC. A FIPS deployment using `credentials-login` is not fully covered.
- `aws-config`'s `sso` and `credentials-login` features hash a start URL or session string with SHA-1 and SHA-256 to name a cache file. Those are RustCrypto and stay that way; they are not security functions.
- A build with `aws-lc-fips` compiles both AWS-LC libraries, because rustls's own `fips` feature stacks on its `aws_lc_rs` feature and so keeps `aws-lc-sys` in the graph. Only the validated one reaches the binary, though: `aws-lc-rs` routes every call to its FIPS build, so nothing references `aws-lc-sys` and the linker drops it. An unstripped release build of an SDK program with this feature contains 2144 `aws_lc_fips_0_14_2_*` symbols and zero `aws_lc_0_45_0_*` symbols. So you pay the non-validated library's build time but do not ship it.
