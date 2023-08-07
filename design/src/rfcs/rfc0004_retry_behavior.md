RFC: Retry Behavior
============================

> Status: Implemented

For a summarized list of proposed changes, see the [Changes Checklist](#changes-checklist) section.

It is not currently possible for users of the SDK to configure a client's maximum number of retry attempts. This RFC establishes a method for users to set the number of retries to attempt when calling a service and would allow users to disable retries entirely. This RFC would introduce breaking changes to the `retry` module of the `aws-smithy-client` crate.

Terminology
-----------

- **Smithy Client**: A `aws_smithy_client::Client<C, M, R>` struct that is responsible for gluing together
  the connector, middleware, and retry policy. This is not generated and lives in the `aws-smithy-client` crate.
- **Fluent Client**: A code-generated `Client<C, M, R>` that has methods for each service operation on it.
  A fluent builder is generated alongside it to make construction easier.
- **AWS Client**: A specialized Fluent Client that defaults to using a `DynConnector`, `AwsMiddleware`,
  and `Standard` retry policy.
- **Shared Config**: An `aws_types::Config` struct that is responsible for storing shared configuration data that is used across all services. This is not generated and lives in the `aws-types` crate.
- **Service-specific Config**: A code-generated `Config` that has methods for setting service-specific configuration. Each `Config` is defined in the `config` module of its parent service. For example, the S3-specific config struct is `use`able from `aws_sdk_s3::config::Config` and re-exported as `aws_sdk_s3::Config`.
- **Standard retry behavior**: The standard set of retry rules across AWS SDKs. This mode includes a standard set of errors that are retried, and support for retry quotas. The default maximum number of attempts with this mode is three, unless `max_attempts` is explicitly configured.
- **Adaptive retry behavior**: Adaptive retry mode dynamically limits the rate of AWS requests to maximize success rate. This may be at the expense of request latency. Adaptive retry mode is not recommended when predictable latency is important.
  - _Note: supporting the "adaptive" retry behavior is considered outside the scope of this RFC_

Configuring the maximum number of retries
------------

This RFC will demonstrate _(with examples)_ the following ways that Users can set the maximum number of retry attempts:

- By calling the `Config::retry_config(..)` or `Config::disable_retries()` methods when building a service-specific config
- By calling the `Config::retry_config(..)` or `Config::disable_retries()` methods when building a shared config
- By setting the `AWS_MAX_ATTEMPTS` environment variable

The above list is in order of decreasing precedence e.g. setting maximum retry attempts with the `max_attempts` builder method will override a value set by `AWS_MAX_ATTEMPTS`.

_The default number of retries is 3 as specified in the [AWS SDKs and Tools Reference Guide](https://docs.aws.amazon.com/sdkref/latest/guide/setting-global-max_attempts.html)._

### Setting an environment variable

Here's an example app that logs your AWS user's identity

```rust,ignore
use aws_sdk_sts as sts;

#[tokio::main]
async fn main() -> Result<(), sts::Error> {
    let config = aws_config::load_from_env().await;

    let sts = sts::Client::new(&config);
    let resp = sts.get_caller_identity().send().await?;
    println!("your user id: {}", resp.user_id.unwrap_or_default());
    Ok(())
}
```

Then, in your terminal:

```sh
# Set the env var before running the example program
export AWS_MAX_ATTEMPTS=5
# Run the example program
cargo run
```

### Calling a method on an AWS shared config

Here's an example app that creates a shared config with custom retry behavior and then logs your AWS user's identity

```rust,ignore
use aws_sdk_sts as sts;
use aws_types::retry_config::StandardRetryConfig;

#[tokio::main]
async fn main() -> Result<(), sts::Error> {
    let retry_config = StandardRetryConfig::builder().max_attempts(5).build();
    let config = aws_config::from_env().retry_config(retry_config).load().await;

    let sts = sts::Client::new(&config);
    let resp = sts.get_caller_identity().send().await?;
    println!("your user id: {}", resp.user_id.unwrap_or_default());
    Ok(())
}
```

### Calling a method on service-specific config

Here's an example app that creates a service-specific config with custom retry behavior and then logs your AWS user's identity

```rust,ignore
use aws_sdk_sts as sts;
use aws_types::retry_config::StandardRetryConfig;

#[tokio::main]
async fn main() -> Result<(), sts::Error> {
    let config = aws_config::load_from_env().await;
    let retry_config = StandardRetryConfig::builder().max_attempts(5).build();
    let sts_config = sts::config::Config::from(&config).retry_config(retry_config).build();

    let sts = sts::Client::new(&sts_config);
    let resp = sts.get_caller_identity().send().await?;
    println!("your user id: {}", resp.user_id.unwrap_or_default());
    Ok(())
}
```

### Disabling retries

Here's an example app that creates a shared config that disables retries and then logs your AWS user's identity

```rust,ignore
use aws_sdk_sts as sts;
use aws_types::config::Config;

#[tokio::main]
async fn main() -> Result<(), sts::Error> {
    let config = aws_config::from_env().disable_retries().load().await;
    let sts_config = sts::config::Config::from(&config).build();

    let sts = sts::Client::new(&sts_config);
    let resp = sts.get_caller_identity().send().await?;
    println!("your user id: {}", resp.user_id.unwrap_or_default());
    Ok(())
}
```

Retries can also be disabled by explicitly passing the `RetryConfig::NoRetries` enum variant to the `retry_config` builder method:

```rust,ignore
use aws_sdk_sts as sts;
use aws_types::retry_config::RetryConfig;

#[tokio::main]
async fn main() -> Result<(), sts::Error> {
    let config = aws_config::load_from_env().await;
    let sts_config = sts::config::Config::from(&config).retry_config(RetryConfig::NoRetries).build();

    let sts = sts::Client::new(&sts_config);
    let resp = sts.get_caller_identity().send().await?;
    println!("your user id: {}", resp.user_id.unwrap_or_default());
    Ok(())
}
```

Behind the scenes
-----------------

Currently, when users want to send a request, the following occurs:

1. The user creates either a shared config or a service-specific config
1. The user creates a fluent client for the service they want to interact with and passes the config they created. Internally, this creates an AWS client with a default retry policy
1. The user calls an operation builder method on the client which constructs a request
1. The user sends the request by awaiting the `send()` method
1. The smithy client creates a new `Service` and attaches a copy of its retry policy
1. The `Service` is `call`ed, sending out the request and retrying it according to the retry policy

After this change, the process will work like this:

1. The user creates either a shared config or a service-specific config
    - If `AWS_MAX_ATTEMPTS` is set to zero, this is invalid and we will log it with `tracing::warn`. However, this will not error until a request is made
    - If `AWS_MAX_ATTEMPTS` is 1, retries will be disabled
    - If `AWS_MAX_ATTEMPTS` is greater than 1, retries will be attempted at most as many times as is specified
    - If the user creates the config with the `.disable_retries` builder method, retries will be disabled
    - If the user creates the config with the `retry_config` builder method, retry behavior will be set according to the `RetryConfig` they passed
1. The user creates a fluent client for the service they want to interact with and passes the config they created
    - Provider precedence will determine what retry behavior is actually set, working like how `Region` is set
1. The user calls an operation builder method on the client which constructs a request
1. The user sends the request by awaiting the `send()` method
1. The smithy client creates a new `Service` and attaches a copy of its retry policy
1. The `Service` is `call`ed, sending out the request and retrying it according to the retry policy

These changes will be made in such a way that they enable us to add the "adaptive" retry behavior at a later date without introducing a breaking change.

Changes checklist
-----------------

- [x] Create new Kotlin decorator `RetryConfigDecorator`
  - Based on [RegionDecorator.kt](https://github.com/awslabs/smithy-rs/blob/main/aws/sdk-codegen/src/main/kotlin/software/amazon/smithy/rustsdk/RegionDecorator.kt)
  - This decorator will live in the `codegen` project because it has relevance outside the SDK
- [x] **Breaking changes:**
  - [x] Rename `aws_smithy_client::retry::Config` to `StandardRetryConfig`
  - [x] Rename `aws_smithy_client::retry::Config::with_max_retries` method to `with_max_attempts` in order to follow AWS convention
  - [x] Passing 0 to `with_max_attempts` will panic with a helpful, descriptive error message
- [x] Create non-exhaustive `aws_types::retry_config::RetryConfig` enum wrapping structs that represent specific retry behaviors
  - [x] A `NoRetry` variant that disables retries. Doesn't wrap a struct since it doesn't need to contain any data
  - [x] A `Standard` variant that enables the standard retry behavior. Wraps a `StandardRetryConfig` struct.
- [x] Create `aws_config::meta::retry_config::RetryConfigProviderChain`
- [x] Create `aws_config::meta::retry_config::ProvideRetryConfig`
- [x] Create `EnvironmentVariableMaxAttemptsProvider` struct
  - Setting AWS_MAX_ATTEMPTS=0 and trying to load from env will panic with a helpful, descriptive error message
- [x] Add `retry_config` method to `aws_config::ConfigLoader`
- [x] Update `AwsFluentClientDecorator` to correctly configure the max retry attempts of its inner `aws_hyper::Client` based on the passed-in `Config`
- [x] Add tests
  - [x] Test that setting retry_config to 1 disables retries
  - [x] Test that setting retry_config to `n` limits retries to `n` where `n` is a non-zero integer
  - [x] Test that correct precedence is respected when overriding retry behavior in a service-specific config
  - [x] Test that correct precedence is respected when overriding retry behavior in a shared config
  - [x] Test that creating a config from env if AWS_MAX_ATTEMPTS=0 will panic with a helpful, descriptive error message
  - [x] Test that setting invalid `max_attempts=0` with a `StandardRetryConfig` will panic with a helpful, descriptive error message
