# AWS Configuration RFC

> Status: Implemented. For an ordered list of proposed changes see: [Proposed changes](#changes-checklist).

An AWS SDK loads configuration from multiple locations. Some of these locations can be loaded synchronously. Some are
async. Others may actually use AWS services such as STS or SSO.

This document proposes an overhaul to the configuration design to facilitate three things:

1. Future-proof: It should be easy to add additional sources of region and credentials, sync and async, from many
   sources, including code-generated AWS services.
2. Ergonomic: There should be one obvious way to create an AWS service client. Customers should be able to easily
   customize the client to make common changes. It should encourage sharing of things that are expensive to create.
3. Shareable: A config object should be usable to configure multiple AWS services.

## Usage Guide

> The following is an imagined usage guide if this RFC where implemented.

### Getting Started

Using the SDK requires two crates:

1. `aws-sdk-<someservice>`: The service you want to use (e.g. `dynamodb`, `s3`, `sesv2`)
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

```rust,ignore
use aws_sdk_dynamodb as dynamodb;

#[tokio::main]
async fn main() -> Result<(), dynamodb::Error> {
    let config = aws_config::load_from_env().await;
    let dynamodb = dynamodb::Client::new(&config);
    let resp = dynamodb.list_tables().send().await;
    println!("my tables: {}", resp.tables.unwrap_or_default());
    Ok(())
}
```

> Tip: Every AWS service exports a top level `Error` type (e.g. [aws_sdk_dynamodb::Error](https://awslabs.github.io/aws-sdk-rust/aws_sdk_dynamodb/enum.Error.html)).
> Individual operations return specific error types that contain only the [error variants returned by the operation](https://awslabs.github.io/aws-sdk-rust/aws_sdk_dynamodb/error/struct.ListTablesError.html).
> Because all the individual errors implement `Into<dynamodb::Error>`, you can use `dynamodb::Error` as the return type along with `?`.

Next, we'll explore some other ways to configure the SDK. Perhaps you want to override the region loaded from the
environment with your region. In this case, we'll want more control over how we load config,
using `aws_config::from_env()` directly:

```rust,ignore
use aws_sdk_dynamodb as dynamodb;

#[tokio::main]
async fn main() -> Result<(), dynamodb::Error> {
    let region_provider = RegionProviderChain::default_provider().or_else("us-west-2");
    let config = aws_config::from_env().region(region_provider).load().await;
    let dynamodb = dynamodb::Client::new(&config);
    let resp = dynamodb.list_tables().send().await;
    println!("my tables: {}", resp.tables.unwrap_or_default());
    Ok(())
}
```

### Sharing configuration between multiple services

The `Config` produced by `aws-config` can be used with any AWS service. If we wanted to read our Dynamodb DB tables
aloud with Polly, we could create a Polly client as well. First, we'll need to add Polly to our `Cargo.toml`:

```toml
[dependencies]
aws-sdk-dynamo = "0.1"
aws-sdk-polly = "0.1"
aws-config = "0.5"

tokio = { version = "1", features = ["full"] }
```

Then, we can use the shared configuration to build both service clients. The region override will apply to both clients:

```rust,ignore
use aws_sdk_dynamodb as dynamodb;
use aws_sdk_polly as polly;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> { // error type changed to `Box<dyn Error>` because we now have dynamo and polly errors
    let config = aws_config::env_loader().with_region(Region::new("us-west-2")).load().await;

    let dynamodb = dynamodb::Client::new(&config);
    let polly = polly::Client::new(&config);

    let resp = dynamodb.list_tables().send().await;
    let tables = resp.tables.unwrap_or_default();
    let table_sentence = format!("my dynamo DB tables are: {}", tables.join(", "));
    let audio = polly.synthesize_speech()
        .output_format(OutputFormat::Mp3)
        .text(table_sentence)
        .voice_id(VoiceId::Joanna)
        .send()
        .await?;

    // Get MP3 data from the response and save it
    let mut blob = resp
        .audio_stream
        .collect()
        .await
        .expect("failed to read data");

    let mut file = tokio::fs::File::create("tables.mp3")
        .await
        .expect("failed to create file");

    file.write_all_buf(&mut blob)
        .await
        .expect("failed to write to file");
    Ok(())
}
```

### Specifying a custom credential provider

If you have your own source of credentials, you may opt-out of the standard credential provider chain.

To do this, implement the `ProvideCredentials` trait.

> NOTE: `aws_types::Credentials` already implements `ProvideCredentials`. If you want to use the SDK with static credentials, you're already done!

```rust,ignore
use aws_types::credentials::{ProvideCredentials, provide_credentials::future, Result};

struct MyCustomProvider;

impl MyCustomProvider {
    pub async fn load_credentials(&self) -> Result {
        todo!() // A regular async function
    }
}

impl ProvideCredentials for MyCustomProvider {
    fn provide_credentials<'a>(&'a self) -> future::ProvideCredentials<'a>
        where
            Self: 'a,
    {
        future::ProvideCredentials::new(self.load_credentials())
    }
}
```

> Hint: If your credential provider is not asynchronous, you can use `ProvideCredentials::ready` instead to save an allocation.

After writing your custom provider, you'll use it in when constructing the configuration:

```rust,ignore
#[tokio::main]
async fn main() {
    let config = aws_config::from_env().credentials_provider(MyCustomProvider).load().await;
    let dynamodb = dynamodb::new(&config);
}
```

## Proposed Design

Achieving this design consists of three major changes:

1. Add a `Config` struct to `aws-types`. This contains a config, but with no logic to _construct_ it. This represents
   what configuration SDKS need, but **not** how to load the information from the environment.
2. Create the `aws-config` crate. `aws-config` contains the logic to load configuration from the environment. No
   generated service clients will depend on `aws-config`. This is critical to avoid circular dependencies and to
   allow `aws-config` to depend on other AWS services. `aws-config` contains individual providers as well as a
   pre-assembled default provider chain for region and credentials. It will also contain crate features to automatically
   bring in HTTPS and async-sleep implementations.
3. Remove all "business logic" from `aws-types`. `aws-types` should be an interface-only crate that is extremely stable.
   The ProvideCredentials trait should move into `aws-types`. The region provider trait which only exists to support
   region-chaining will move out of aws-types into aws-config.

Services will continue to generate their own `Config` structs. These will continue to be customizable as they are today,
however, they won't have any default resolvers built in. Each AWS config will implement `From<&aws_types::SharedConfig>`
. A convenience method to `new()` a fluent client directly from a shared config will also be generated.

### Shared Config Implementation

This RFC proposes adding region and credentials providers support to the shared config. A future RFC will propose
integration with HTTP settings, HTTPs connectors, and async sleep.

```rust,ignore
struct Config {
    // private fields
    ...
}

impl Config {
    pub fn region(&self) -> Option<&Region> {
        self.region.as_ref()
    }

    pub fn credentials_provider(&self) -> Option<SharedCredentialsProvider> {
        self.credentials_provider.clone()
    }

    pub fn builder() -> Builder {
        Builder::default()
    }
}

```

The `Builder` for `Config` allows customers to provide individual overrides and handles the insertion of the default
chain for regions and credentials.

### Sleep + Connectors

Sleep and Connector are both runtime dependent features. `aws-config` will define `rt-tokio` and `rustls`
and `native-tls` optional features. **This centralizes the Tokio/Hyper dependency** eventually removing the need for
each service to maintain their own Tokio/Hyper features.

Although not proposed in this RFC, shared config will eventually gain support for creating an HTTPs client from HTTP
settings.

## The `.build()` method on <service>::Config

Currently, the `.build()` method on service config will fill in defaults. As part of this change, `.build()` called on
the service config with missing properties will fill in "empty" defaults. If no credentials provider is given,
a `NoCredentials` provider will be set, and `Region` will remain as `None`.

## Stability and Versioning

The introduction of `Config` to aws-types is not without risks. If a customer depends on a version aws-config that
uses `Config` that is incompatible, they will get confusing compiler errors.

An example of a problematic set of dependent versions:

```markdown
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

To mitigate this risk, we will need to make `aws-types` essentially permanently stable. Changes to `aws-types` need to
be made with extreme care. This will ensure that two versions of `aws-types` never end up in a customer's dependency
tree.

We will dramatically reduce the surface area of `aws-types` to contain only interfaces.

Several breaking changes will be made as part of this, notably, the profile file parsing will be moved out of aws-types.

Finally, to mitigate this risk even further, services will `pub use` items from `aws-types` directly which means that
even if a dependency mismatch exists, it is still possible for customers to work around it.

## Changes Checklist

- [x] ProvideRegion becomes async using a newtype'd future.
- [x] AsyncProvideCredentials is removed. ProvideCredentials becomes async using a newtype'd future.
- [x] ProvideCredentials moved into `aws-types`. `Credentials` moved into `aws-types`
- [x] Create `aws-config`.
- [x] Profile-file parsing moved into `aws-config`, region chain & region environment loaders moved to `aws-config`.
- [ ] os_shim_internal moved to ??? `aws-smithy-types`?
- [x] Add `Config` to `aws-types`. Ensure that it's set up to add new members while remaining backwards
  compatible.
- [x] Code generate `From<&SharedConfig> for <everyservice>::Config`
- [x] Code generate `<everservice>::Client::new(&shared_config)`
- [x] Remove `<everyservice>::from_env`

## Open Issues
- [ ] Connector construction needs to be a function of HTTP settings
- [ ] An AsyncSleep should be added to `aws-types::Config`
