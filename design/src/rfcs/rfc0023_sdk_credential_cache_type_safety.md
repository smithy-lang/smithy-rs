RFC: SDK Credential Cache Type Safety
=====================================

> Status: RFC
>
> Applies to: AWS SDK for Rust

At time of writing (2022-10-11), the SDK's credentials provider can be customized by providing:

1. A profile credentials file to modify the default provider chain
2. An instance of one of the credentials providers implemented in `aws-config`, such as
   the `AssumeRoleCredentialsProvider`, `ImdsCredentialsProvider`, and so on.
3. A custom struct that implements the `ProvideCredentials`

The problem this RFC examines is that when options 2 and 3 above are exercised, the customer
needs to be aware of credentials caching and put additional effort to ensure caching is set up
correctly (and that double caching doesn't occur). This is especially difficult to get right
since some built-in credentials providers (such as `AssumeRoleCredentialsProvider`) already
have caching, while most others do not and need to be wrapped in `LazyCachingCredentialsProvider`.

The goal of this RFC is to create an API where Rust's type system ensures caching is set up
correctly, or explicitly opted out of.

Option A: `ProvideCachedCredentials` trait
--------------------------------------------

In this option, `aws-types` has a `ProvideCachedCredentials` in addition to `ProvideCredentials`.
All individual credential providers (such as `ImdsCredentialsProvider`) implement `ProvideCredentials`,
while credential caches (such as `LazyCachingCredentialsProvider`) implement the `ProvideCachedCredentials`.
The `ConfigLoader` would only take `impl ProvideCachedCredentials`.

This allows customers to provide their own caching solution by implementing `ProvideCachedCredentials`,
while requiring that caching be done correctly through the type system since `ProvideCredentials` is
only useful inside the implementation of `ProvideCachedCredentials`.

Caching can be opted out by creating a `NoCacheCredentialsProvider` that implements `ProvideCachedCredentials`
without any caching logic, although this wouldn't be recommended and this provider wouldn't be vended
in `aws-config`.

Example configuration:
```rust
// Compiles
let sdk_config = aws_config::from_env()
    .credentials(
        LazyCachingCredentialsProvider::builder()
            .load(ImdsCredentialsProvider::new())
            .build()
    )
    .load()
    .await;

// Doesn't compile
let sdk_config = aws_config::from_env()
    // Wrong type: doesn't implement `ProvideCachedCredentials`
    .credentials(ImdsCredentialsProvider::new())
    .load()
    .await;
```

Another method could be added to `ConfigLoader` that makes it easier to use the default cache:

```rust
let sdk_config = aws_config::from_env()
    .credentials_with_default_cache(ImdsCredentialsProvider::new())
    .load()
    .await;
```

### Pros/cons

- :+1: It's flexible, and somewhat enforces correct cache setup through types.
- :+1: Removes the possibility of double caching since the cache implementations won't
  implement `ProvideCredentials`.
- :-1: Customers may unintentionally implement `ProvideCachedCredentials` instead of `ProvideCredentials`
  for a custom provider, and then not realize they're not benefiting from caching.
- :-1: The documentation needs to make it very clear what the differences are between `ProvideCredentials`
  and `ProvideCachedCredentials` since they will look identical.
- :-1: It's possible to implement both `ProvideCachedCredentials` and `ProvideCredentials`, which
  breaks the type safety goals.

Option B: `CacheCredentials` trait
----------------------------------

This option is similar to Option A, except that the cache trait is distinct from `ProvideCredentials` so
that it's more apparent when mistakenly implementing the wrong trait for a custom credentials provider.

A `CacheCredentials` trait would be added that looks as follows:
```rust
pub trait CacheCredentials: Send + Sync + Debug {
    pub async fn cached(&self, now: SystemTime) -> Result<Credentials, CredentialsError>;
}
```

Instances implementing `CacheCredentials` need to own the `ProvideCredentials` implementation
to make both lazy and eager credentials caching possible.

The configuration examples look identical to Option A.

### Pros/cons

- :+1: It's flexible, and enforces correct cache setup through types slightly better than Option A.
- :+1: Removes the possibility of double caching since the cache implementations won't
  implement `ProvideCredentials`.
- :-1: Customers can still unintentionally implement the wrong trait and miss out on caching when
  creating custom credentials providers, but it will be more apparent than in Option A.
- :-1: It's possible to implement both `CacheCredentials` and `ProvideCredentials`, which
  breaks the type safety goals.

Option C: `CredentialsCache` enum
---------------------------------

The enum approach posits that customers don't need or want to implement custom credential caching,
but at the same time, doesn't make it impossible to add custom caching later.

The idea is that there would be an enum called `CredentialsCache` that specifies the desired
caching approach for a given credentials provider:

```rust
pub struct LazyCache {
    credentials_provider: Arc<dyn ProvideCredentials>,
    // ...
}

pub struct EagerCache {
    credentials_provider: Arc<dyn ProvideCredentials>,
    // ...
}

pub struct CustomCache {
    credentials_provider: Arc<dyn ProvideCredentials>,
    // Not naming or specifying the custom cache trait for now since its out of scope
    cache: Arc<dyn SomeCacheTrait>
}

enum CredentialsCacheInner {
    Lazy(LazyCache),
    // Eager doesn't exist today, so this is purely for illustration
    Eager(EagerCache),
    // Custom may not be implemented right away
    Custom(CustomCache),
}

pub struct CredentialsCache {
    inner: CredentialsCacheInner,
}

impl CredentialsCache {
    // Methods prefixed with `default_` just use the default cache settings
    pub fn default_lazy(provider: impl ProvideCredentials + 'static) -> Self { /* ... */ }
    pub fn default_eager(provider: impl ProvideCredentials + 'static) -> Self { /* ... */ }

    // Unprefixed methods return a builder that can take customizations
    pub fn lazy(provider: impl ProvideCredentials + 'static) -> LazyBuilder { /* ... */ }
    pub fn eager(provider: impl ProvideCredentials + 'static) -> EagerBuilder { /* ... */ }

    pub(crate) fn create_cache(
        self,
        sleep_impl: Arc<dyn AsyncSleep>
    ) -> SharedCredentialsProvider {
        // ^ Note: SharedCredentialsProvider would get renamed to SharedCredentialsCache.
        // This code is using the old name to make it clearer that it already exists,
        // and the rename is called out in the change checklist.
        SharedCredentialsProvider::new(
            match self {
                Self::Lazy(inner) => LazyCachingCredentialsProvider::new(inner.credentials_provider, settings.time, /* ... */),
                Self::Eager(_inner) => unimplemented!(),
                Self::Custom(_custom) => unimplemented!(),
            }
        )
    }
}
```

Using an enum over a trait prevents custom caching implementations, but if customization is desired,
a `Custom` variant could be added that has its own trait that customers implement.

The `SharedCredentialsProvider` needs to be updated to take a cache implementation rather
than `impl ProvideCredentials + 'static`. A sealed trait could be added to facilitate this.

Configuration would look as follows:

```rust
let sdk_config = aws_config::from_env()
    .credentials(CredentialsCache::default_lazy(ImdsCredentialsProvider::builder().build()))
    .load()
    .await;
```

The `credentials_provider` method on `ConfigLoader` would only take `CredentialsCache` as an argument
so that the SDK could not be configured without credentials caching, or if opting out of caching becomes
a use case, then a `CredentialsCache::NoCache` variant could be made.

Like Option A, a convenience method can be added to make using the default cache easier:

```rust
let sdk_config = aws_config::from_env()
    .credentials_with_default_cache(ImdsCredentialsProvider::builder().build())
    .load()
    .await;
```

In the future if custom caching is added, it would look as follows:

```rust
let sdk_config = aws_config::from_env()
    .credentials(
        CredentialsCache::custom(ImdsCredentialsProvider::builder().build(), MyCache::new())
    )
    .load()
    .await;
```

The `ConfigLoader` wouldn't be able to immediately set its credentials provider since other values
from the config are needed to construct the cache (such as `sleep_impl`). Thus, the `credentials`
setter would merely save off the `CredentialsCache` instance, and then when `load` is called,
the complete `SharedCredentialsProvider` would be constructed:

```rust
pub async fn load(self) -> SdkConfig {
    // ...
    let credentials_provider = self.credentials_cache.create_cache(sleep_impl);
    // ...
}
```

### Pros/cons

- :+1: Removes the possibility of missing out on caching when implementing a custom provider.
- :+1: Removes the possibility of double caching since the cache implementations won't
  implement `ProvideCredentials`.
- :-1: Requires a lot of boilerplate in `aws-config` for the builders, enum variant structs, etc.

Commonalities
-------------

All approaches propose renaming the `credentials_provider` method on `ConfigLoader` to
`credentials` since it will take a cache instead of a provider. This also makes the proposed
`credentials_with_default_cache` method name shorter.

Recommendation
--------------

Option C should be taken since it offers the greatest type safety while still maintaining
enough flexibility.

Changes Checklist
-----------------

Option C:
- [ ] Implement `CredentialsCache` with its `Lazy` variant and builder
- [ ] Refactor `ConfigLoader` to take `CredentialsCache` instead of `impl ProvideCredentials + 'static`
- [ ] Refactor `SharedCredentialsProvider` to take a cache implementation instead of `impl ProvideCredentials + 'static`
- [ ] Add `ConfigLoader::credentials_with_default_cache` method
- [ ] Renames:
  - [ ] `ConfigLoader::credentials_provider` -> `ConfigLoader::credentials`
  - [ ] `SharedCredentialsProvider` -> `SharedCredentialsCache`
  - [ ] `LazyCachingCredentialsProvider` -> `LazyCredentialsCache`
- [ ] Refactor the SDK `Config` code generator to be consistent with `ConfigLoader`
- [ ] Write changelog upgrade instructions
- [ ] Fix examples
