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

## Usage Guide

> The following is proposal of the usage guide customers for customers using the AWS SDK.

### Getting Started

Using the SDK requires two crates:

1. `aws-sdk-<someservice>`: The service you want to use (eg. `dynamodb`, `s3`, `sesv2`)
2. `aws-config`: AWS metaconfiguration. This crate contains all the of logic to load configuration for the SDK (regions,
   credentials, retry configuration, etc.)

Add the following to your Cargo.toml:

```toml
[dependencies]
aws-sdk-dynamo = "0.1"
aws-config = "0.5"

tokio = { version = "1", features = ["full"] }
```

Let's write a small example project to list tables:

```rust
use aws_sdk_dynamodb as dynamodb;

#[tokio::main]
async fn main() -> Result<(), dynamodb::Error> {
    let config = aws_config::load_config_from_environment().await;
    let dynamodb = dynamodb::Client::new(&config);
    let resp = dynamodb.list_tables().send().await;
    println!("my tables: {}", resp.tables.unwrap_or_default());
    Ok(())
}
```

> Tip: Every AWS service exports a top level `Error` type (eg. [aws_sdk_dynamodb::Error](https://awslabs.github.io/aws-sdk-rust/aws_sdk_dynamodb/enum.Error.html)).
> Individual operations return specific error types that contain only the [error variants returned by the operation](https://awslabs.github.io/aws-sdk-rust/aws_sdk_dynamodb/error/struct.ListTablesError.html).
> Because all the individual errors implement `Into<dynamodb::Error>`, you can use `dynamodb::Error` as the return type along with `?`.

Next, we'll explore some other ways to configure the SDK. Perhaps you want to override the region loaded from the
environment with your region. In this case, we'll want more control over how we load config,
using `aws_config::env_loader()` directly:

```rust
use aws_sdk_dynamodb as dynamodb;

#[tokio::main]
async fn main() -> Result<(), dynamodb::Error> {
    let config = aws_config::env_loader().with_region(Region::new("us-west-2")).await;
    let dynamodb = dynamodb::Client::new(&config);
    let resp = dynamodb.list_tables().send().await;
    println!("my tables: {}", resp.tables.unwrap_or_default());
    Ok(())
}
```

### Sharing a config between multiple services

The `Config` produced by `aws-config` doesn't just work with DynamoDB, but with any AWS service. If we wanted to read
our Dynamodb DB tables aloud with Polly, we could create a Polly client as well.

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

To mitigate this risk, we will instead introduce `aws-types`. `aws-core` is intended to be permanently stable. Changes
to `aws-types` need to be made with extreme care. Likely, `aws-core` will launch at `0.1` and only experience minor
version bumps until a single breaking change at 1.0.

This will ensure that two versions of `aws-types` never end up in a customer's dependency tree.

Several breaking changes will be made as part of this, notably, the config file parsing will be moved out of aws-types.

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
