# AWS Configuration RFC

> Status: RFC. For an ordered list of proposed changes see: [Proposed changes](#changes-checklist).

An AWS SDK loads configuration from multiple locations. Some of these locations can be loaded synchronously. Some are
async. Others may actually use AWS services such as STS or SSO.

This document proposes an overhaul to the Config design to facilitate three things:

1. Future-proof: It should be easy to add additional sources of region and credentials, sync and async, from many
   sources, including code generated services.
2. Ergonomic: There should be one obvious way to create an AWS service client. Customers should be able to easily
   customize the client to make common changes. It should lead customers into the pit of success in terms of sharing
   things that are expensive to create.
3. Shareable: A config object should be usable to configure multiple AWS services.

## New customer experience

With the new shared config, customers must always take `aws-config` as a dependency if they want access to the automatic
configuration loading.

In Cargo.toml:

```toml
[dependencies]
aws-sdk-dynamo = "0.1"
aws-config = "0.5"
```

When creating a client, customers will create a `SharedConfig` struct, then use it to construct the service client.
The `SharedConfig` struct is defined centrally in `aws-types`. (See [stability](#stability-and-versioning))

```rust
// pub use of aws_types::SharedConfig
use aws_config::SharedConfig;

async fn main() {
    // loading a shared config may make network calls, etc.
    // with some custom configuration:
    let config = aws_config::config_loader()
            .region(Region::new("us-east-1"))
            .sleep(async_std_sleep)
            .load()
            .await;

    // with no custom configuration:
    let config = aws_config::load_config().await;
    // does not consume config so that config can be used to construct other service clients.
    let client = aws_sdk_dynamodb::Client::new(&config);
    let tables = client.list_tables().await?;
}
```

## Proposed Design

Achieving this design consists of three high level changes:

1. Add a `SharedConfig` struct to `aws-types`. This contains a config, but with no logic to be loaded from the
   environment in any way.
2. Add an `aws-config` crate. `aws-config` contains the logic to load configuration from the environment. No generated
   service clients will depend on `aws-config`. This is critical to avoid circular dependencies and to
   allow `aws-config` to depend on other AWS services. `aws-config` will contain a AWS-generic configuration
   representation + default resolver chains.
3. Remove all "business logic" from `aws-types`. `aws-types` should be an interface-only crate that is long term
   extremely stable. The credential provider interface should move into `aws-types`.

Services will continue to have their own `Config` structs. These will continue to be customizable as they are today,
however, they won't have any default resolvers built in. Each AWS config will implement `From<&aws_types::SharedConfig>`
.

```rust
struct SharedConfig {
    ...
}

impl SharedConfig {
    pub fn new_connector(&self, version: HttpVersion) -> Option<DynConnector> {
        todo!()
    }

    pub fn credentials_provider(&self) -> impl ProvideCredentials {
        todo!()
    }

    pub fn region(&self) -> Option<Region> {
        todo!()
    }

    pub fn sleep(&self) -> impl Sleep {
        todo!()
    }

    pub fn retry_config(&self) ->...// eg. mode, max attempts etc.

    pub fn timeout_config(&self) -> // eg. request timeout, read timeout, etc.
}
```

The `Builder` for `SharedConfig` allows customers to provide individual overrides and handles the insertion of the
default chain for regions, sleep, connectors, and credentials.

## Sleep + Connector

Sleep and Connector are both runtime dependent features. `aws-config` will use `tokio` and `hyper` as optional
features. **This centralizes the Tokio/Hyper dependency** removing the need for each service to maintain their own
Tokio/Hyper features.

## The `.build()` method on service config

Currently, the `.build()` method on service config will fill in defaults. As part of this change, `.build()` called on
the service config with missing properties would be a runtime panic. Most customers would utilize the `From`
implementation.

## Stability and Versioning

The introduction of `SharedConfig` to aws-types is not without risks. If a customer depends on a version aws-config that
uses `SharedConfig` that is incompatible, they will get confusing compiler errors.

An example of a problematic version set:

```
┌─────────────────┐                 ┌───────────────┐
│ aws-types = 0.1 │                 │aws-types= 0.2 │
└─────────────────┘                 └───────────────┘
           ▲                                 ▲
           │                                 │
           │                                 │
           │                                 │
 ┌─────────┴─────────────┐          ┌────────┴───────┐
 │aws-sdk-dynamodb = 0.5 │          │aws-config = 0.6│
 └───────────┬───────────┘          └───────┬────────┘
             │                              │
             │                              │
             │                              │
             │                              │
             │                              │
             ├─────────────────────┬────────┘
             │ my-lambda-function  │
             └─────────────────────┘
```

To mitigate this risk, we will need to make `aws-types` essentially permanently stable. Changes
to `aws-types` need to be made with extreme care. This will ensure that two versions of `aws-types` never end up in a customer's dependency tree.

We will dramatically reduce the surface area of `aws-types` to contain only interfaces.

Several breaking changes will be made as part of this, notably, the profile file parsing will be moved out of aws-types.

## Changes Checklist

- [ ] ProvideRegion becomes async using a newtype'd future.
- [ ] AsyncProvideCredentials is removed. ProvideCredentials becomes async using a newtype'd future.
- [ ] ProvideCredentials moved into `aws-types`. `Credentials` moved into `aws-types`
- [ ] Create `aws-config`.
- [ ] Profile-file parsing moved into `aws-config`, region chain & region environment loaders moved to `aws-config`.
- [ ] os_shim_internal moved to ??? `smithy-types`?
- [ ] Add `SharedConfig` to `aws-types`. Ensure that it's set up to add new members while remaining backwards
  compatible.
- [ ] Code generate `From<&SharedConfig> for <everyservice>::Config`
- [ ] Code generate `<everservice>::Client::new(&shared_config)`
- [ ] Deprecate `<everyservice>::from_env`, message points to `Client::new`
- [ ] Remove `<everyservice>::from_env`
