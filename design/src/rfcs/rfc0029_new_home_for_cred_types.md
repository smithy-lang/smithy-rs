RFC: Finding New Home for Credential Types
===================================================

> Status: Implemented in [smithy-rs#2108](https://github.com/awslabs/smithy-rs/pull/2108)
>
> Applies to: clients

This RFC supplements [RFC 28](https://github.com/awslabs/smithy-rs/blob/main/design/src/rfcs/rfc0028_sdk_credential_cache_type_safety.md) and discusses for the selected design where to place the types for credentials providers, credentials caching, and everything else that comes with them.

It is assumed that the primary motivation behind the introduction of type safe credentials caching remains the same as the preceding RFC.

Assumptions
-----------
This document assumes that the following items in the changes checklist in the preceding RFC have been implemented:
- [x] Implement `CredentialsCache` with its `Lazy` variant and builder
- [x] Add the `credentials_cache` method to `ConfigLoader`
- [x] Rename `SharedCredentialsProvider` to `SharedCredentialsCache`
- [x] Remove `ProvideCredentials` impl from `LazyCachingCredentialsProvider`
- [x] Rename `LazyCachingCredentialsProvider` -> `LazyCredentialsCache`
- [x] Refactor the SDK `Config` code generator to be consistent with `ConfigLoader`

Problems
--------
Here is how our attempt to implement the selected design in the preceding RFC can lead to an obstacle. Consider this code snippet we are planning to support:

```rust,ignore
let sdk_config = aws_config::from_env()
    .credentials_cache(CredentialsCache::lazy())
    .load()
    .await;

let client = aws_sdk_s3::Client::new(&sdk_config);
```

A `CredentialsCache` created by `CredentialsCache::lazy()` above will internally go through three crates before the variable `client` has been created:
1. `aws-config`: after it has been passed to `aws_config::ConfigLoader::credentials_cache`
```rust,ignore
// in lib.rs

impl ConfigLoader {
    // --snip--
    pub fn credentials_cache(mut self, credentials_cache: CredentialsCache) -> Self {
        self.credentials_cache = Some(credentials_cache);
        self
    }
    // --snip--
}
```
2. `aws-types`: after `aws_config::ConfigLoader::load` has passed it to `aws_types::sdk_config::Builder::credentials_cache`
```rust,ignore
// in sdk_config.rs

impl Builder {
    // --snip--
    pub fn credentials_cache(mut self, cache: CredentialsCache) -> Self {
        self.set_credentials_cache(Some(cache));
        self
    }
    // --snip--
}
```
3. `aws-sdk-s3`: after `aws_sdk_s3::Client::new` has been called with the variable `sdk_config`
```rust,ignore
// in client.rs

impl Client {
    // --snip--
    pub fn new(sdk_config: &aws_types::sdk_config::SdkConfig) -> Self {
        Self::from_conf(sdk_config.into())
    }
    // --snip--
}
```
calls
```rust,ignore
// in config.rs

impl From<&aws_types::sdk_config::SdkConfig> for Builder {
    fn from(input: &aws_types::sdk_config::SdkConfig) -> Self {
        let mut builder = Builder::default();
        builder = builder.region(input.region().cloned());
        builder.set_endpoint_resolver(input.endpoint_resolver().clone());
        builder.set_retry_config(input.retry_config().cloned());
        builder.set_timeout_config(input.timeout_config().cloned());
        builder.set_sleep_impl(input.sleep_impl());
	builder.set_credentials_cache(input.credentials_cache().cloned());
        builder.set_credentials_provider(input.credentials_provider().cloned());
        builder.set_app_name(input.app_name().cloned());
        builder.set_http_connector(input.http_connector().cloned());
        builder
    }
}

impl From<&aws_types::sdk_config::SdkConfig> for Config {
    fn from(sdk_config: &aws_types::sdk_config::SdkConfig) -> Self {
        Builder::from(sdk_config).build()
    }
}
```
What this all means is that `CredentialsCache` needs to be accessible from `aws-config`, `aws-types`, and `aws-sdk-s3` (SDK client crates, to be more generic). We originally assumed that `CredentialsCache` would be defined in `aws-config` along with `LazyCredentialsCache`, but the assumption no longer holds because `aws-types` and `aws-sdk-s3` do not depend upon `aws-config`.

Therefore, we need to find a new place in which to create credentials caches accessible from the aforementioned crates.

Proposed Solution
-----------------
We propose to move the following items to a new crate called `aws-credential-types`:
- All items in `aws_types::credentials` and their dependencies
- All items in `aws_config::meta::credentials` and their dependencies

For the first bullet point, we move types and traits associated with credentials out of `aws-types`. Crucially, the `ProvideCredentials` trait now lives in `aws-credential-types`.

For the second bullet point, we move the items related to credentials caching. `CredentialsCache` with its `Lazy` variant and builder lives in `aws-credential-types` and `CredentialsCache::create_cache` will be marked as `pub`. One area where we make an adjustment, though, is that `LazyCredentialsCache` depends on `aws_types::os_shim_internal::TimeSource` so we need to move `TimeSource` into `aws-credentials-types` as well.

A result of the above arrangement will give us the following module dependencies (only showing what's relevant):
<p align="center">
  <img width="800" alt="Selected design" src="https://user-images.githubusercontent.com/15333866/207222687-8dd2430c-2865-4161-85bc-ee1410040d38.png">
<p>

- :+1: `aws_types::sdk_config::Builder` and a service client `config::Builder` can create a `SharedCredentialsCache` with a concrete type of credentials cache.
- :+1: It avoids cyclic crate dependencies.
- :-1: There is one more AWS runtime crate to maintain and version.

Rejected Alternative
--------------
An alternative design is to move the following items to a separate crate (tentatively called `aws-XXX`):
- All items in `aws_types::sdk_config`, i.e. `SdkConfig` and its builder
- All items in `aws_types::credentials` and their dependencies
- All items in `aws_config::meta::credentials` and their dependencies

The reason for the first bullet point is that the builder needs to be somewhere it has access to the credentials caching factory function, `CredentialsCache::create_cache`. The factory function is in `aws-XXX` and if the builder stayed in `aws-types`, it would cause a cyclic dependency between those two crates.

A result of the above arrangement will give us the following module dependencies:
<p align="center">
  <img width="800" alt="Option A" src="https://user-images.githubusercontent.com/15333866/206587781-6eca3662-5096-408d-a435-d4023929e727.png">
</p>

We have dismissed this design mainly because we try moving out of the `aws-types` create as little as possible. Another downside is that `SdkConfig` sitting together with the items for credentials provider & caching does not give us a coherent mental model for the `aws-XXX` crate, making it difficult to choose the right name for `XXX`.

Changes Checklist
-----------------
The following list does not repeat what is listed in the preceding RFC but does include those new mentioned in the `Assumptions` section:

- [ ] Create `aws-credential-types`
- [ ] Move all items in `aws_types::credentials` and their dependencies to the `aws-credential-types` crate
- [ ] Move all items in `aws_config::meta::credentials` and their dependencies to the `aws-credential-types` crate
- [ ] Update use statements and fully qualified names in the affected places
