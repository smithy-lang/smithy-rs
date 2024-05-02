RFC: Identity Cache Partitions
===============================

> Status: Implemented
>
> Applies to: AWS SDK for Rust

Motivation
-----------

In the below example two clients are created from the same shared `SdkConfig` instance and each
invoke a fictitious operation. Assume the operations use the same auth scheme relying on the same identity resolver.

```rust,ignore
#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {

    let config = aws_config::defaults(BehaviorVersion::latest())
        .load()
        .await;

    let c1 = aws_sdk_foo::Client::new(&config);
    c1.foo_operation().send().await;

    let c2 = aws_sdk_bar::Client::new(&config);
    c2.bar_operation().send().await;

    Ok(())
}
```

There are two problems with this currently.


1. The identity resolvers (e.g. `credentials_provider` for SigV4) are re-used but we end up with a different
[`IdentityCachePartition`](https://github.com/smithy-lang/smithy-rs/blob/release-2024-03-25/rust-runtime/aws-smithy-runtime-api/src/client/identity.rs#L41)
each time a client is created.
    * More specifically this happens every time a `SharedIdentityResolver` is [created](https://github.com/smithy-lang/smithy-rs/blob/release-2024-03-25/rust-runtime/aws-smithy-runtime-api/src/client/identity.rs#L197). The conversion from [`From<SdkConfig>`](https://github.com/awslabs/aws-sdk-rust/blob/release-2024-04-01/sdk/sts/src/config.rs#L1207)
    sets the credentials provider which [associates](https://github.com/awslabs/aws-sdk-rust/blob/release-2024-04-01/sdk/sts/src/config.rs#L960) it as
    the identity resolver for the auth scheme. Internally this is [converted](https://github.com/awslabs/aws-sdk-rust/blob/release-2024-04-01/sdk/aws-smithy-runtime-api/src/client/runtime_components.rs#L663) to `SharedIdentityResolver` which creates the new partition (if it were already a `SharedIdentityResolver` this would be detected and a new instance would not be created which means it must be a `SharedCredentialsProvider` or `SharedTokenProvider` that is getting converted). The end result is the credentials
    provider from shared config is re-used but the cache partition differs so a cache miss occurs the first time
    any new client created from that shared config needs credentials.

2. The `SdkConfig` does not create an identity cache by default. Even if the partitioning is fixed, any clients created from
a shared config instance will end up with their own identity cache which also results in having to resolve identity
again. Only if a user supplies an identity cache explicitly when creating shared config would it be re-used across
different clients.

### Design intent

Identity providers and identity caching are intentionally decoupled. This allows caching behavior to be more easily
customized and centrally configured while also removing the need for each identity provider to have to implement
caching. There is some fallout from sharing an identity cache though. This is fairly well documented on
`IdentityCachePartition` itself.

```rust,ignore
/// ...
///
/// Identities need cache partitioning because a single identity cache is used across
/// multiple identity providers across multiple auth schemes. In addition, a single auth scheme
/// may have many different identity providers due to operation-level config overrides.
///
/// ...
pub struct IdentityCachePartition(...)
```

Cache partitioning allows for different identity types to be stored in the same cache instance as long as they
are assigned to a different partition. Partitioning also solves the issue of overriding configuration on a per operation
basis where it would not be the correct or desired behavior to re-use or overwrite the cache if a different resolver
is used.

In other words cache partitioning is effectively tied to a particular instance of an identity resolver. Re-using the
same instance of a resolver _SHOULD_ be allowed to share a cache partition. The fact that this isn't the case
today is an oversight in how types are wrapped and threaded through the SDK.


The user experience if this RFC is implemented
----------------------------------------------

In the current version of the SDK, users are unable to share cached results of identity resolvers via shared `SdkConfig`
across clients.

Once this RFC is implemented, users that create clients via `SdkConfig` with the latest behavior version will share
a default identity cache. Shared identity resolvers (e.g. `credentials_provider`, `token_provider`, etc) will provide
their own cache partition that is re-used instead of creating a new one each time a provider is converted into a
`SharedIdentityResolver`.

### Default behavior

```rust,ignore
let config = aws_config::defaults(BehaviorVersion::latest())
    .load()
    .await;

let c1 = aws_sdk_foo::Client::new(&config);
c1.foo_operation().send().await;


let c2 = aws_sdk_bar::Client::new(&config);
// will re-use credentials/identity resolved via c1
c2.bar_operation().send().await;
```

Operations invoked on `c2` will see the results of cached identities resolved by client `c1` (for operations that use
the same identity resolvers). The creation of a default identity cache in `SdkConfig` if not provided will be added
behind a new behavior version.

### Opting out

Users can disable the shared identity cache by explicitly setting it to `None`. This will result in each client
creating their own identity cache.

```rust,ignore
let config = aws_config::defaults(BehaviorVersion::latest())
    // new method similar to `no_credentials()` to disable default cache setup
    .no_identity_cache()
    .load()
    .await;

let c1 = aws_sdk_foo::Client::new(&config);
c1.foo_operation().send().await;


let c2 = aws_sdk_bar::Client::new(&config);
c2.bar_operation().send().await;
```

The same can be achieved by explicitly supplying a new identity cache to a client:

```rust,ignore

let config = aws_config::defaults(BehaviorVersion::latest())
    .load()
    .await;

let c1 = aws_sdk_foo::Client::new(&config);
c1.foo_operation().send().await;

let modified_config = aws_sdk_bar::Config::from(&config)
    .to_builder()
    .identity_cache(IdentityCache::lazy().build())
    .build();

// uses it's own identity cache
let c2 = aws_sdk_bar::Client::from_conf(modified_config);
c2.bar_operation().send().await;
```

### Interaction with operation config override

How per/operation configuration override behaves depends on what is provided for an identity resolver.

```rust,ignore
let config = aws_config::defaults(BehaviorVersion::latest())
    .load()
    .await;

let c1 = aws_sdk_foo::Client::new(&config);

let scoped_creds = my_custom_provider();
let config_override = c1
        .config()
        .to_builder()
        .credentials_provider(scoped_creds);

// override config for two specific operations

c1.operation1()
    .customize()
    .config_override(config_override);
    .send()
    .await;

c1.operation2()
    .customize()
    .config_override(config_override);
    .send()
    .await;
```

By default if an identity resolver does not provide it's own cache partition then `operation1` and `operation2` will
be wrapped in new `SharedIdentityResolver` instances and get distinct cache partitions. If `my_custom_provider()`
provides it's own cache partition then `operation2` will see the cached results.

Users can control this by wrapping their provider into a `SharedCredentialsProvider` which will claim it's own
cache partition.

```rust,ignore

let scoped_creds = SharedCredentialsProvider::new(my_custom_provider());
let config_override = c1
        .config()
        .to_builder()
        .set_credentials_provider(Some(scoped_creds));
...
```


How to actually implement this RFC
----------------------------------

In order to implement this RFC implementations of `ResolveIdentity` need to be allowed to provide their own cache
partition.

```rust,ignore
pub trait ResolveIdentity: Send + Sync + Debug {
    ...

    /// Returns the identity cache partition associated with this identity resolver.
    ///
    /// By default this returns `None` and cache partitioning is left up to `SharedIdentityResolver`.
    /// If sharing instances of this type should use the same partition then you should override this
    /// method and return a claimed partition.
    fn cache_partition(&self) -> Option<IdentityCachePartition> {
        None
    }

}
```

Crucially cache partitions must remain globally unique so this method returns `IdentityCachePartition` which is
unique by construction. It doesn't matter if partitions are claimed early by an implementation of `ResolveIdentity`
or at the time they are wrapped in `SharedIdentityResolver`.

This is because `SdkConfig` stores instances of `SharedCredentialsProvider` (or `SharedTokenProvider`) rather than
`SharedIdentityResolver` which is what currently knows about cache partitioning. By allowing implementations of `ResolveIdentity`
to provide their own partition then `SharedCredentialsProvider` can claim a partition at construction time
and return that which will re-use the same partition anywhere that the provider is shared.


```rust,ignore
#[derive(Clone, Debug)]
pub struct SharedCredentialsProvider(Arc<dyn ProvideCredentials>, IdentityCachePartition);

impl SharedCredentialsProvider {
    pub fn new(provider: impl ProvideCredentials + 'static) -> Self {
        Self(Arc::new(provider), IdentityCachePartition::new())
    }
}

impl ResolveIdentity for SharedCredentialsProvider {
    ...

    fn cache_partition(&self) -> Option<IdentityCachePartition> {
        Some(self.1)
    }
}

```

Additionally a new behavior version must be introduced that conditionally creates a default `IdentityCache` on `SdkConfig`
if not explicitly configured (similar to how credentials provider works internally).

Alternatives Considered
-----------------------

`SdkConfig` [internally](https://github.com/smithy-lang/smithy-rs/blob/release-2024-03-25/aws/rust-runtime/aws-types/src/sdk_config.rs#L58-L59)
stores `SharedCredentialsProvider`/`SharedTokenProvider`. Neither of these types knows anything about cache partitioning.
One alternative would be to create and store a `SharedIdentityResolver` for each identity resolver type.

```rust,ignore
pub struct SdkConfig {
    ...
    credentials_provider: Option<SharedCredentialsProvider>,
    credentials_identity_provider: Option<SharedIdentityResolver>,
    token_provider: Option<SharedTokenProvider>,
    token_identity_provider: Option<SharedIdentityResolver>,
}
```

Setting one of the identity resolver types like `credentials_provider` would also create and set the equivalent
`SharedIdentityResolver` which would claim a cache partition. When generating the `From<SdkConfig>` implementations
the identity resolver type would be favored.

There are a few downsides to this approach:

1. `SdkConfig` would have to expose accessor methods for the equivalents
(e.g. `credentials_identity_provider(&self) -> Option<&SharedIdentityResolver>`). This creates additional noise
and confusion as well as the chance for using the type wrong.
2. Every new identity type added to `SdkConfig` would have to be sure to use `SharedIdentityResolver`.

The advantage of the proposed approach of letting `ResolveIdentity` implementations provide a cache partition means
`SdkConfig` does not need to change. It also gives customers more control over whether an identity resolver implementation
shares a cache partition or not.

Changes checklist
-----------------

- [x] Add new `cache_partition()` method to `ResolveIdentity`
- [x] Update `SharedIdentityResolver::new` to use the new `cache_partition()` method on the `resolver` to determine if a new cache partition should be created or not
- [x] Claim a cache partition when `SharedCredentialsProvider` is created and override the new `ResolveIdentity` method
- [x] Claim a cache partition when `SharedTokenProvider` is created and override the new `ResolveIdentity` method
- [x] Introduce new behavior version
- [x] Conditionally (gated on behavior version) create a new default `IdentityCache` on `SdkConfig` if not explicitly configured
- [x] Add a new `no_identity_cache()` method to `ConfigLoader` that marks the identity cache as explicitly unset
