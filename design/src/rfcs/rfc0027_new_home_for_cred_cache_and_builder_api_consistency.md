RFC: Finding New Home for Credential Cache and Ensuring Consistency for Builder API
=====================================================================================

> Status: RFC
>
> Applies to: clients

This RFC supplements [smithy-rs#1842](https://github.com/awslabs/smithy-rs/pull/1842) and discusses for the selected design where to place the types and traits for credentials caching and everything else that comes with it. It also discusses how to make the methods `credentials_provider` and `credentials_cache` behave consistently across `ConfigLoader`, `sdk_config::Builder`, and service clients' `config::Builder`s.

It is assumed that the primary motivation behind the introduction of type safe credentials caching remains the same as the preceding RFC.

Assumptions
-----------
This document assumes that the following items in the changes checklist in the preceding RFC have been implemented.
- [x] Implement `CredentialsCache` with its `Lazy` variant and builder
- [x] Add the `credentials_cache` method to `ConfigLoader`
- [x] Rename `SharedCredentialsProvider` to `SharedCredentialsCache`
- [x] Remove `ProvideCredentials` impl from `LazyCachingCredentialsProvider`
- [x] Rename `LazyCachingCredentialsProvider` -> `LazyCredentialsCache`

This document also assumes that the following changes have been made that are not explicitly stated in the changes checklist:
- [x] Add a new `pub` trait `ProvideCachedCredentials` in the `aws-types` crate that has a single method `fn provide_cached_credentials<'a>(&'a self) -> future::ProvideCredentials<'a> where Self: 'a`
- [x] Update `SharedCredentialsCache` to be a newtype wrapper around `Arc<dyn ProvideCachedCredentials>`
- [x] Implement `ProvideCachedCredentials` for `SharedCredentialsCache`
- [x] Implement `ProvideCachedCredentials` for `LazyCredentialsCache`

The reason for the additional changes is that we will need to put a credentials cache into a property bag and get it from the bag in `CredentialsStage`, i.e. in `aws_http::auth` we will have
```rust
// formerly known as `set_provider`
pub fn set_credentials_cache(bag: &mut PropertyBag, cache: SharedCredentialsCache) {
    bag.insert(cache);
}

impl CredentialsStage {
    // --snip--

    async fn load_creds(mut request: Request) -> Result<Request, CredentialsStageError> {
        let credentials_cache = request
            .properties()
            .get::<SharedCredentialsCache>()
            .cloned();

        // --snip--

        match credentials_cache.provide_cached_credentials().await {
            Ok(creds) => {
                request.properties_mut().insert(creds);
            }
            // --snip--
        }

        // --snip--
    }
}
```

Problems
--------
Here is how our attempt to implement the selected design in the preceding RFC led to a dead end. Consider this code snippet we are planning to support:

```rust
let sdk_config = aws_config::from_env()
    .credentials_cache(CredentialsCache::lazy())
    .load()
    .await;
```

This implies that `aws_config::ConfigLoader::load` will internally have something as follows:

```rust
pub async fn load(self) -> SdkConfig {
    // --snip--
    let credentials_provider = if let Some(provider) = self.credentials_provider {
        provider
    } else {
        // --snip--
    };

    // We're still massaging the input parameters for `create_cache`
    // and there could be more parameters, but what's important is
    // that the method's return type is `SharedCredentialsCache`.
    let credentials_cache: SharedCredentialsCache =
        self.credentials_cache.create_cache(credentials_provider);

    // --snip--

    let mut builder = SdkConfig::builder()
        .region(region)
        .retry_config(retry_config)
        .timeout_config(timeout_config)
        .credentials_cache(credentials_cache) // new method
        .http_connector(http_connector);

    // --snip--

    builder.build()
}
```

The comment `// new method` above further implies that `aws_types::sdk_config::Builder` now has a method `credentials_cache` that takes a `SharedCredentialCache`. What this means is that `aws_types::sdk_config::Builder` has both `credentials_provider` and `credentials_cache` methods. This presents two problems.

-  `aws_types::sdk_config::Builder` has to drop on the floor a credentials provider passed to the `credentials_provider` method.

    There is another credentials provider already baked in a credentials cache passed from `aws_config::ConfigLoader::load`. `aws_types::sdk_config::Builder` has no way of creating another `SharedCredentialsCache` using the passed-in credentials provider to overwrite that credentials cache for the reason explained in the next problem.

- Outside the `aws-config` crate, the user who directly interacts with `aws_types::sdk_config::Builder` has no way to prepare a `SharedCredentialCache` to pass it to the `credentials_cache` method.

	The factory function for credentials caches, `CredentialsCache::create_cache`, is `pub(crate)` so the caches can only be created within the `aws-config` crate. Even if the factory function were marked as `pub`, the places that can produce the caches would still be limited to those that do not cause a cyclic crate dependency with respect to the `aws-config` crate, which excludes the `aws-types` crate for instance.

Note that the same set of problems exists for a service client's `config::Builder`. Such a `Builder` has the methods `credentials_provider` and `set_credentials_provider`. Therefore, it is reasonable to expect that the `Builder` will also have the methods `credentials_cache` and `set_credentials_cache`, except that the assumption does not hold as the service client crate, being outside the `aws-config` crate, cannot create a credentials cache, unable to pass it to the said methods.

To summarize, these are the problems encountered as a result of implementation spike for the selected design in the preceding RFC:
- `ConfigLoader` has `credentials_provider` and `credentials_cache`, but that pair of methods does not seem to behave consistently in `aws_types::sdk_config::Builder` and every service client's `config::Builder`.
- A credentials cache can only be created within the `aws-config` crate.

Proposed Solutions
------------------
In this section we will talk about our proposed solution separately for each of the bullet points from the previous section, one for API consistency and one for creating credentials caches.

### API consistency for `credentials_provider` and `credentials_cache` in other builders
We propose to have `sdk_config::Builder` and service clients' `config::Builder`s match `ConfigLoader` and take the `credentials_cache` method to make it consistent all the way through.

- :+1: The user can expect a pair of `credentials_provider` and `credentials_cache` methods behaves consistently across `ConfigLoader`,  `sdk_config::Builder` and service clients' `config::Builder`s.
- :-1: It will require some duplicate code across those places, although it's rather an insignificant amount.

### Creating credentials caches outside the `aws-config` crate
We propose to move types and traits for credentials caching to a separate crate (tentatively called `aws-XXX`). Specifically, the following items will be moved to `aws-XXX`:
- All items in `aws_types::credentials` and their dependencies
- All items in `aws_config::meta::credentials` and their dependencies

The new crate will be the home for `ProvideCachedCredentials` discussed above as well as `CredentialsCache` with its `Lazy` variant and builder. Crucially, `CredentialsCache::create_cache` will be marked as `pub`. The `aws-XXX` crate itself depends on `aws_smithy_types` and `aws_smithy_async`, among others, for the items in it to be able to compile. There is one wrinkle, however, that is `LazyCredentialsCache` depends on `aws_types::os_shim_internal::TimeSource`; it does not feel right to move `TimeSource` to `aws-XXX` just because `LazyCredentialsCache` requires it. There are two ways to work around it.

#### Option A: Moving `aws_types::sdk_config` to `aws-XXX`

A result of this arrangement will give us the following module interdependencies:
<p align="center">
  <img width="800" alt="Option A" src="https://user-images.githubusercontent.com/15333866/206587781-6eca3662-5096-408d-a435-d4023929e727.png">
</p>

- :+1: `aws-XXX::sdk_config::Builder` and a service client `config::Builder` can create a `SharedCredentialsCache` with a concrete type of credentials cache.
- :+1: It avoids cyclic crate dependencies.
- :-1: There is one more crate to maintain and version.
- :-1: `aws-XXX::sdk_config` sitting together with the types and traits for credentials provider & caching may not give us a coherent mental model for the `aws-XXX` crate, making it difficult to choose the right name for `XXX`.

#### Option B: Moving `aws_types::sdk_config` to yet another new crate

A result of this arrangement will give us the following module interdependencies:
<p align="center">
  <img width="800" alt="Option B" src="https://user-images.githubusercontent.com/15333866/206587806-fe720118-e3d4-4dc9-9b33-5f65fc1a445c.png">
</p>

- :+1: `aws-XXX::sdk_config::Builder` and a service client `config::Builder` can create a `SharedCredentialsCache` with a concrete type of credentials cache.
- :+1: It avoids cyclic crate dependencies.
- :+1: The scope of `aws-XXX` is more focused, concerned primarily with credentials.
- :-1: There are two more crates to maintain and version.
- :-1: :-1: The scope of the new crate for `aws_types::sdk_config` seems too narrow and therefore having a separate crate may not be worth it.


Rejected Ideas
--------------
### API consistency for `credentials_provider` and `credentials_cache` in other builders

Keeping the credentials caching code in the `aws-config` crate, we considered replacing the `credentials_provider` method with `credentials_cache` method in  `aws_types::sdk_config::Builder` and service clients' `config::Builder`s. This will help constrain the use of the APIs in that `ConfigLoader` is the only place that can configure a credentials provider and a credentials cache; `sdk_config::Builder` and service client's `config::Builder` can only receive a `SharedCredentialsCache` already constructed by `ConfigLoader`. But the constraint is a bit too much because those who directly interact with `sdk_config::Builder` and `config::Builder` cannot create a credentials cache, making those builders pretty much unusable.

### Creating credentials caches outside the `aws-config` crate

There have been no rejected alternatives discovered because the code for credentials caching needs to go somewhere outside the `aws-config` crate anyway. Existing crate dependencies act as constraints and have forced us to place credentials caching and providers as described in the proposed solution.

Changes Checklist
-----------------
The following list does not repeat what is listed in the preceding RFC but does include those new mentioned in the `Assumptions` section:

- [ ] Create and name the `aws-XXX` crate
- [ ] Move all items in `aws_types::credentials` and their dependencies to the `aws-XXX` crate
- [ ] Move all items in `aws_config::meta::credentials` and their dependencies to the `aws-XXX` crate
- [ ] Add a new `pub` trait `ProvideCachedCredentials` in the `aws-XXX` crate that has a single method `fn provide_cached_credentials<'a>(&'a self) -> future::ProvideCredentials<'a> where Self: 'a`
- [ ] Update `SharedCredentialsCache` to be a newtype wrapper around `Arc<dyn ProvideCachedCredentials>`
- [ ] Implement `ProvideCachedCredentials` for `SharedCredentialsCache`
- [ ] Implement `ProvideCachedCredentials` for `LazyCredentialsCache`
- [ ] Add `credentials_cahce` & `set_credentials_cache` to both `sdk_config::Builder` and service clients' `config::Builder`s
- [ ] Move `aws_types::sdk_config` to the appropriate place based on either option A or option B discussed above.
- [ ] Make changes to `CredentialsStage` accordingly using `SharedCredentialsCache`
