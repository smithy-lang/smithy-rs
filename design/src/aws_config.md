# AWS Configuration RFC

An AWS SDK loads configuration from many different location. Some of these locations can be loaded synchronously. Some
are async. Others may actually use AWS services.

This document proposes an overhaul to the Config design to facilitate three things:

1. Future-proof: It should be easy to add additional sources of region and credentials, sync and async, from many
   sources, including code generated services.
2. Ergonomic: There should be one obvious way to create an AWS service client.
3. Shareable: A config object should be usable to configure multiple AWS services.

## New customer experience

In Cargo.toml:

```toml
[dependencies]
aws-sdk-dynamo = "0.1"
aws-config = "0.5"
```

```rust
// pub use of aws_types::SharedConfig
use aws_config::SharedConfig;
async fn main() {
   // loading a shared config may make network calls, etc.
   let config = SharedConfig::load_from_environment().await;
   // does not consume config
   let client = aws_sdk_dynamodb::Client::new(&config);
   let _ = client.list_tables().await?;
}

```

## Proposed Design

We propose two changes:

1. Add a `SharedConfig` struct to `aws-types`. This contains a config, but with no logic to be loaded from the
   environment in any way.
2. Add an `aws-config` crate. `aws-config` contains the logic to load configuration from the environment. No generated
   service clients will depend on `aws-config`. This is critical to avoid circular dependencies and to
   allow `aws-config` to depend on other AWS services. `aws-config` will contain a AWS-generic configuration
   representation + default resolver chains.

Services will continue to have their own `Config` structs. These will continue to be customizable as they are today,
however, they won't have any default resolvers built in. Each AWS config will
implement `From<&aws_types::SharedConfig>`.

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

    pub fn timeout_config(&self) ->
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
