RFC: Retry Behavior
============================

> Status: RFC

For a summarized list of proposed changes, see the [Changes Checklist](#changes-checklist) section.

It is not currently possible for users of the SDK to configure a client's retry behavior. This RFC establishes a method for users to set the number of retries to attempt when calling a service and would allow users to disable retries entirely.

Terminology
-----------

- **Shared Config**: An `aws_config::Config` struct that is responsible for storing shared configuration data that is used across all services. This is not generated and lives in the `aws-config` crate.
- **Service-specific Config**: A code-generated `Config` that has methods for setting service-specific configuration. Each `Config` is defined in the `config` module of its parent service. For example, the S3-specific config struct is `use`able from `aws_sdk_s3::config::Config` and re-exported as `aws_sdk_s3::Config`.

Retry config
------------

This RFC will demonstrate _(with examples)_ the following ways that Users can set the number of retries:

- By calling the retry_config method on a service-specific config
- By calling the `aws_config::ConfigLoader::retry_config(..)` method
- By setting the `AWS_MAX_ATTEMPTS` environment variable

The above list is in order of decreasing precedence e.g. setting retry attempts with the `retry_config` method will override a value set by `AWS_MAX_ATTEMPTS`.

_The default number of retries is 3 as specified in the [AWS SDKs and Tools Reference Guide](https://docs.aws.amazon.com/sdkref/latest/guide/setting-global-max_attempts.html)._

### Setting an environment variable

Here's an example app that logs your AWS user's identity

```rust
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

```rust
use aws_sdk_sts as sts;
use aws_types::config::Config;
use aws_types::retry_config::RetryConfig;

#[tokio::main]
async fn main() -> Result<(), sts::Error> {
    let config = aws_config::from_env()
        .max_attempts(5)
        .load().await;

    let sts = sts::Client::new(&config);
    let resp = sts.get_caller_identity().send().await?;
    println!("your user id: {}", resp.user_id.unwrap_or_default());
    Ok(())
}
```

### Calling a method on service-specific config

Here's an example app that creates a service-specific config with custom retry behavior and then logs your AWS user's identity

```rust
use aws_sdk_sts as sts;
use aws_types::config::Config;
use aws_types::retry_config::RetryConfig;

#[tokio::main]
async fn main() -> Result<(), sts::Error> {
    let config = aws_config::load_from_env().await;
    let sts_config = sts::config::Config::from(&config).max_attempts(5).build();

    let sts = sts::Client::new(&sts_config);
    let resp = sts.get_caller_identity().send().await?;
    println!("your user id: {}", resp.user_id.unwrap_or_default());
    Ok(())
}
```

### Disabling retries

Here's an example app that creates a service-specific config with custom retry behavior disabling retries and then logs your AWS user's identity

```rust
use aws_sdk_sts as sts;
use aws_types::config::Config;
use aws_types::retry_config::RetryConfig;

#[tokio::main]
async fn main() -> Result<(), sts::Error> {
    let config = aws_config::load_from_env().await;
    let sts_config = sts::config::Config::from(&config).max_attempts(0).build();

    let sts = sts::Client::new(&sts_config);
    let resp = sts.get_caller_identity().send().await?;
    println!("your user id: {}", resp.user_id.unwrap_or_default());
    Ok(())
}
```

Behind the scenes
-----------------

// TODO

Changes Checklist
-----------------

- [ ] Create new Kotlin decorator `RetryDecorator`
  - Based on [RegionDecorator.kt](https://github.com/awslabs/smithy-rs/blob/main/aws/sdk-codegen/src/main/kotlin/software/amazon/smithy/rustsdk/RegionDecorator.kt)
- [ ] Create `aws_types::max_attempts::MaxAttempts` struct and corresponding builder with a `max_attempts` setter.
- [ ] Create `aws_config::meta::max_attempts::MaxAttemptsProviderChain`
- [ ] Create `aws_config::meta::max_attempts::ProvideMaxAttempts`
- [ ] Create `EnvironmentVariableMaxAttemptsProvider` struct
- [ ] Add `max_attempts` method to `aws_config::ConfigLoader`
- [ ] Update `AwsFluentClientDecorator` to correctly configure the retry behavior of its inner `aws_hyper::Client` based on the retry config.
- [ ] Add tests
  - [ ] Test that setting max_attempts to 0 disables retries
  - [ ] Test that setting max_attempts to `n` limits retries to `n` where `n` is a non-zero integer
