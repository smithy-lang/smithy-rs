# RFC: How Cargo "features" should be used in the SDK and runtime crates

> Status: Accepted

## Some background on features

What is a feature? Here's a definition from the [Cargo Book section on features]:

> Cargo "features" provide a mechanism to express conditional compilation and optional dependencies. A package defines a set of named features in the `[features]` table of `Cargo.toml`, and each feature can either be enabled or disabled. Features for the package being built can be enabled on the command-line with flags such as `--features`. Features for dependencies can be enabled in the dependency declaration in `Cargo.toml`.

We use features in a majority of our runtime crates and in all of our SDK crates. For example, [aws-sigv4] uses them to enable event streams. Another common use case is exhibited by [aws-sdk-s3] which uses them to enable the `tokio` runtime and the TLS implementation used when making requests.

### Features should be additive

The Cargo book has this to say:

> When a dependency is used by multiple packages, Cargo will use the union of all features enabled on that dependency when building it. This helps ensure that only a single copy of the dependency is used.

> A consequence of this is that features should be *additive*. That is, enabling a feature should not disable functionality, and it should usually be safe to enable any combination of features. **A feature should not introduce a [SemVer-incompatible change].**

## What does this mean for the SDK?

Despite the constraints outlined above, we should use features in the SDKs because of the benefits they bring:

- Features enable users to avoid compiling code that they won't be using. Additionally, features allow both general and specific control of compiled code, serving the needs of both novice and expert users.
- A single feature in a crate can activate or deactivate multiple features exposed by that crate's dependencies, freeing the user from having to specifically activate or deactivate them.
- Features can help users understand what a crate is capable of in the same way that looking at a graph of a crate's modules can.

When using features, we should adhere to the guidelines outlined below.

### Avoid writing code that relies on only activating one feature from a set of mutually exclusive features.

As noted earlier in an excerpt from the Cargo book:

> enabling a feature should not disable functionality, and it should usually be safe to enable any combination of features. A feature should not introduce a [SemVer-incompatible change].

```rust
#[cfg(feature = "__rustls")]
impl<M, R> ClientBuilder<(), M, R> {
    /// Connect to the service over HTTPS using Rustls.
    pub fn tls_adapter(self) -> ClientBuilder<Adapter<crate::conns::Https>, M, R> {
        self.connector(Adapter::builder().build(crate::conns::https()))
    }
}

#[cfg(feature = "native-tls")]
impl<M, R> ClientBuilder<(), M, R> {
    /// Connect to the service over HTTPS using the native TLS library on your platform.
    pub fn tls_adapter(
        self,
    ) -> ClientBuilder<Adapter<hyper_tls::HttpsConnector<hyper::client::HttpConnector>>, M, R> {
        self.connector(Adapter::builder().build(crate::conns::native_tls()))
    }
}
```

When the example code above is compiled with both features enabled, compilation will fail with a "duplicate definitions with name `tls_adapter`" error. Also, note that the return type of the function differs between the two versions. This is a SemVer-incompatible change.

Here's an updated version of the example that fixes these issues:

```rust
#[cfg(feature = "__rustls")]
impl<M, R> ClientBuilder<(), M, R> {
    /// Connect to the service over HTTPS using Rustls.
    pub fn rustls(self) -> ClientBuilder<Adapter<crate::conns::Https>, M, R> {
        self.connector(Adapter::builder().build(crate::conns::https()))
    }
}

#[cfg(feature = "native-tls")]
impl<M, R> ClientBuilder<(), M, R> {
    /// Connect to the service over HTTPS using the native TLS library on your platform.
    pub fn native_tls(
        self,
    ) -> ClientBuilder<Adapter<hyper_tls::HttpsConnector<hyper::client::HttpConnector>>, M, R> {
        self.connector(Adapter::builder().build(crate::conns::native_tls()))
    }
}
```

Both features can now be enabled at once without creating a conflict. Since both methods have different names, it's now Ok for them to have different return types.

[*This is real code, see it in context*](https://github.com/smithy-lang/smithy-rs/blob/2e7ed943513203f1472f2490866dc4fb8a392bd3/rust-runtime/aws-smithy-client/src/hyper_ext.rs#L303)

### We should avoid using `#[cfg(not(feature = "some-feature"))]`

At the risk of seeming repetitive, the Cargo book says:

> enabling a feature should not disable functionality, and it should usually be safe to enable any combination of features

Conditionally compiling code when a feature is **not** activated can make it hard for users and maintainers to reason about what will happen when they activate a feature. This is also a sign that a feature may not be "additive".

***NOTE***: It's ok to use `#[cfg(not())]` to conditionally compile code based on a user's OS. It's also useful when controlling what code gets rendered when testing or when generating docs.

One case where using `not` is acceptable is when providing a fallback when no features are set:

```rust,ignore
#[cfg(feature = "rt-tokio")]
pub fn default_async_sleep() -> Option<Arc<dyn AsyncSleep>> {
    Some(sleep_tokio())
}

#[cfg(not(feature = "rt-tokio"))]
pub fn default_async_sleep() -> Option<Arc<dyn AsyncSleep>> {
    None
}
```

### Don't default to defining "default features"

Because Cargo will use the union of all features enabled on a dependency when building it, we should be wary of marking features as default. Once we do mark features as default, users that want to exclude code and dependencies brought in by those features will have a difficult time doing so. One need look no further than [this issue][remove rustls from crate graph] submitted by a user that wanted to use Native TLS and struggled to make sure that Rustls was actually disabled *(This issue was resolved in [this PR][remove default features from runtime crates] which removed default features from our runtime crates.)* This is not to say that we should never use them, as having defaults for the most common use cases means less work for those users.

#### When a default feature providing some functionality is disabled, active features must not automatically replace that functionality

As the SDK is currently designed, the TLS implementation in use can change depending on what features are pulled in. Currently, if a user disables `default-features` (which include `rustls`) and activates the `native-tls` feature, then we automatically use `native-tls` when making requests. For an example of what this looks like from the user's perspective, [see this example][native-tls example].

This RFC proposes that we should have a single default for any configurable functionality and that that functionality depends on a corresponding default feature being active. If `default-features` are disabled, then so is the corresponding default functionality. In its place would be functionality that fails fast with a message describing why it failed *(a default was deactivated but the user didn't set a replacement)*, and what the user should do to fix it *(with links to documentation and examples where necessary)*. We should use [compile-time errors] to communicate failures with users, or `panic`s for cases that can't be evaluated at compile-time.

For an example: Say you have a crate with features `a`, `b`, `c` that all provide some version of functionality `foo`. Feature `a` is part of `default-features`. When `no-default-features = true` but features `b` and `c` are active, don't automatically fall back to `b` or `c`. Instead, emit an error with a message like this:

> "When default features are disabled, you must manually set `foo`. Features `b` and `c` active; You can use one of those. See an example of setting a custom `foo` here: *link-to-docs.amazon.com/setting-foo*"

## Further reading

- [How to tell what "features" are available per crate?]
- [How do I 'pass down' feature flags to subdependencies in Cargo?]
- A small selection of feature-related GitHub issues submitted for popular crates
    - [The feature `preserve_order` is not "purely additive," which makes it impossible to use `serde_yaml` 0.5.0 and `clap` in the same program][yaml-rust#44]
    - [cargo features (verbose-errors may be other?) should be additive][nom#544]
    - [Mutually exclusive features are present in profiling-procmacros][profiling#32]
    - [Clang-sys features not additive][clang-sys#128]

[aws-sigv4]: https://github.com/smithy-lang/smithy-rs/blob/5a1990791d727652587df51b77df4d1df9058252/aws/rust-runtime/aws-sigv4/Cargo.toml
[aws-sdk-s3]: https://github.com/awslabs/aws-sdk-rust/blob/f2b4361b004ee822960ea9791f566fd4eb6d1aba/sdk/s3/Cargo.toml
[Cargo Book section on features]: https://doc.rust-lang.org/cargo/reference/features.html
[SemVer-incompatible change]: https://doc.rust-lang.org/cargo/reference/features.html#semver-compatibility
[remove rustls from crate graph]: https://github.com/awslabs/aws-sdk-rust/issues/304
[remove default features from runtime crates]: https://github.com/smithy-lang/smithy-rs/pull/935
[cfg! macro]: https://doc.rust-lang.org/rust-by-example/attribute/cfg.html
[How to tell what "features" are available per crate?]: https://stackoverflow.com/questions/59761045/how-to-tell-what-features-are-available-per-crate
[How do I 'pass down' feature flags to subdependencies in Cargo?]: https://stackoverflow.com/questions/40021555/how-do-i-pass-down-feature-flags-to-subdependencies-in-cargo
[yaml-rust#44]: https://github.com/chyh1990/yaml-rust/issues/44
[nom#544]: https://github.com/Geal/nom/issues/544
[profiling#32]: https://github.com/aclysma/profiling/issues/32
[clang-sys#128]: https://github.com/KyleMayes/clang-sys/issues/128
[compile-time errors]: https://doc.rust-lang.org/stable/std/macro.compile_error.html
[native-tls example]: https://github.com/smithy-lang/smithy-rs/tree/bc316a0b81b75a00c389f6281a66eb0f5357172a/aws/sdk/examples/using_native_tls_instead_of_rustls
