RFC: Client configuration for the orchestrator
=======================

> Status: RFC
>
> Applies to: client

This RFC proposes two areas of improvement for configuring an SDK client:
- Support for operation-level configuration
- Support for runtime components required by the orchestrator

We have encountered use cases where configuring a client for a single operation invocation is necessary ([example](https://github.com/awslabs/aws-sdk-rust/issues/696)). At the time of writing, this feature is not yet supported, but operation-level configuration will address that limitation.

As described in [RFC 34](https://github.com/awslabs/smithy-rs/blob/main/design/src/rfcs/rfc0034_smithy_orchestrator.md), the orchestrator uses configured components for an SDK to handle messages between the client and a service. They include:
- `RetryStrategy`: Configures how requests are retried.
- `TraceProbes`: Configures locations to which SDK metrics are published.
- `EndpointProviders`: Configures which hostname an SDK will call when making a request.
- `HTTPClients`: Configures how remote services are called.
- `IdentityProviders: Configures how users identify themselves to remote services.
- `HTTPAuthSchemes` & `AuthSchemeResolver`s: Configures how users authenticate themselves to remote services.
- `Checksum Algorithms`: Configures how an SDK calculates request and response checksums.
- `Interceptors`: Configures specific stages of the request execution pipeline.

However, not all of these components are covered by the public APIs for configuring them, and for those that are, they do not appear under exactly the same names, e.g. [endpoint_url](https://docs.rs/aws-config/0.54.1/aws_config/struct.ConfigLoader.html#method.endpoint_url) and [retry_config](https://docs.rs/aws-config/0.54.1/aws_config/struct.ConfigLoader.html#method.retry_config). This RFC proposes allowing users to configure the above components either by keeping current APIs or adding new ones.

What this RFC does NOT cover:
- Interceptors: A future RFC will provide a detailed description of this feature
- Step-by-step instructions for how to migrate current default runtime components to the orchestrator

Terminology
-----------

- Component: An interface coupled with its default implementations to enable an SDK client functionality.
- Fluent Client: A code generated Client that has methods for each service operation on it. A fluent builder is generated alongside it to make construction easier.
- Operation: A high-level abstraction representing an interaction between an SDK Client and a remote service.
- Orchestrator: The code within an SDK client that handles the process of making requests and receiving responses from remote services.
- Remote Service: A remote API that a user wants to use. Communication with a remote service usually happens over HTTP. The remote service is usually, but not necessarily, an AWS service.
- SDK Client: A client generated for the AWS SDK, allowing users to make requests to remote services.


The user experience if this RFC is implemented
----------------------------------------------


### Operation-level configuration

Currently, users are able to customize runtime configuration at multiple levels. `SdkConfig` is used to configure settings for all services, while a service config (e.g., `aws_sdk_s3::config::Config`) is used to configure settings for a specific service client.

With this RFC, users will be able to go one step further and override configuration for a single operation invocation:
```rust
let sdk_config = aws_config::from_env().load().await;
let s3_client = aws_sdk_s3::client::Client::new(&sdk_config);
s3_client.create_bucket()
    .bucket(bucket_name)
    .config_override(aws_sdk_s3::config::builder().region("us-west-1"))
    .send()
    .await;
```

This is achieved through the `.config_override` method, which is added to fluent builders, such as `fluent_builders::CreateBucket`. This method sets the `us-west-1` region to override any region setting specified in the service level config. The operation level config takes the highest precedence, followed by the service level config, and then the AWS level config.

The `config_override` method takes a service config builder instead of a service config, allowing `None` values to be used for fields, so as not to override settings at a lower-precedence configuration.

The main benefit of this approach is simplicity for users. The only change required is to call the `config_override` method on an operation input fluent builder. If users do not wish to specify the operation level config, their workflow will remain unaffected.

### Configuring runtime components required by the orchestrator

While `ConfigLoader`, `SdkConfig`, and service configs allow users to configure the necessary runtime components for today's `Tower`-based infrastructure, they do not cover all of the components required for the orchestrator to perform its job.

The following table shows for each runtime component (the left column), what method on `ConfigLoader`, `sdk_config::Builder`, and service config builder (e.g. `aws_sdk_s3::config::Builder`) are currently available (the middle column) and what new method will be available on those types as proposed by the RFC (the right column).

| Runtime component | Today's builder method | Proposed builder method |
| :-: | --- | --- |
| RetryStrategy | `.retry_config` | `.retry_strategy(&self, impl RetryStrategy + 'static)` |
| TraceProbes | None | (See the Changes checklist) |
| EndpointResolver | `.endpoint_url` | No change |
| HTTPClients | `.http_connector` | No change |
| IdentityProviders | `.credentials_cache` & `.credentials_provider` | No change |
| HTTPAuthSchemes & AuthSchemeResolvers | None | `.auth_scheme(&self, impl HttpAuthScheme + 'static)` |
| Checksum Algorithms | `.checksum_algorithm` only at the operation level for those that support it | No change |
| Interceptors | None | `.interceptor<Res, Req>(&self, impl Interceptor<Res, Req> + 'static)` |

The proposed methods generally take a type that implements [the corresponding trait from RFC 34](https://github.com/awslabs/smithy-rs/blob/main/design/src/rfcs/rfc0034_smithy_orchestrator.md#traits). However, for backward compatibility reasons, we mark "No change" for `EndpointResolver`, `HttpClients`, and `IdentityProviders`:
- `EndpointResolver` - we [deprecated the previous the .endpoint_resolver method](https://github.com/awslabs/smithy-rs/pull/2464). Introducing a new `.endpoint_resolver` might run counter to Endpoints 2.0 migration.
- `HttpClients` - users have been able to specify custom connections using the existing method, and introducing a new method like `.connection(&self, impl Connection + 'static)` might require non-trivial upgrades.
- `IdentityProviders` - users have been able to specify credentials cache and provider in a flexible manner using the existing methods, and introducing a new method like `.identity_provider(&self, impl IdentityProvider + 'static)` might require non-trivial upgrades.

We also marked "No change" for `Checksum Algorithms` because it should not be arbitrarily configurable at the service level config. Today, operations that support a predefined set of checksum algorithms expose the `checksum_algorithm` method through their fluent builders.

How to actually implement this RFC
----------------------------------
Implementing this RFC is tied to [RFC 34](https://github.com/awslabs/smithy-rs/blob/main/design/src/rfcs/rfc0034_smithy_orchestrator.md). In that RFC, applying client configuration means putting runtime components and required function parameters into a type map for `aws_smithy_runtime::client::orchestrator::invoke` to use.

This section covers three parts:
- Where we store operation-level config
- How we put runtime components and required function parameters into a type map
- How we plan to migrate today's runtime components to the world of the orchestrator

#### Where we store operation-level config

Given that fluent builders will have the `configure_override` method, it makes sense for operation-level configuration to be stored in fluent builders. The code generator will be updated to add the following field to fluent builders:
```rust
config_override: Option<service config builder> // e.g. Option<aws_sdk_s3::config::Builder>
```
The field is of type `Option`, so `None` means a case where a user did not specify the operation-level runtime configuration at all. With that, the `config_override` method will look as follows, also added by the code generator to fluent builders:
```rust
pub fn config_override(
    mut self,
    config_override: impl Into<service config burilder>,
) -> Self {
    self.config_override = Some(config_override.into());
    self
}
```

#### How we put runtime components and required function parameters into a type map

First, a high-level picture of the layered configuration needs to be reviewed. When a user has executed the following code:
```rust
let sdk_config = aws_config::from_env().load().await;
let s3_client = aws_sdk_s3::client::Client::new(&sdk_config);
let fluent_builder = s3_client.list_buckets()
    .bucket(bucket_name)
    .config_override(aws_sdk_s3::config::builder().region("us-west-1"));
```
The relations between the types can be illustrated in the following diagram:

<img width="1367" alt="client config at different levels" src="https://user-images.githubusercontent.com/15333866/228719737-d58d5e0a-f7ca-46d4-bb22-33c7c528ba69.png">

The `aws_sdk_s3::config::Config` type on the left stores the service-level configuration, which implicitly includes/shadows the AWS-level configuration. `Config` is accessible via `Handle` from a fluent builder `ListBucketsFluentBuilder`, which holds the operation level config.

After the `send` method is called on the `fluent_builder` variable and before it internally calls `aws_smithy_runtime::client::orchestrator::invoke`, the fields in `self.handle.conf` and those in `self.config_override` will be put into a type map. How this is done exactly is an implementation detail outside the scope of the RFC, but we have been working on [send_v2](https://github.com/awslabs/smithy-rs/blob/b023426d1cfd05e1fd9eef2f92a21cad58b93b86/codegen-client/src/main/kotlin/software/amazon/smithy/rust/codegen/client/smithy/generators/client/FluentClientGenerator.kt#L331-L347) to allow for a gradual transition.

We use the `aws_smithy_runtime_api::config_bag::ConfigBag` type as the type map, and we have defined a trait called `aws_smithy_runtime_api::client::runtime_plugin::RuntimePlugin` that allows trait implementors to add key-value pairs to `ConfigBag`. The `RuntimePlugin` trait is defined as follows:
```rust
pub trait RuntimePlugin {
    fn configure(&self, cfg: &mut ConfigBag) -> Result<(), BoxError>;
}
```

With this in mind, we plan to
- Implement the `RuntimePlugin` trait for service configuration, which will put runtime components and required function parameters specified at the service level config into `ConfigBag`
- Implement the `RuntimePlugin` trait for service configuration builders, which will put runtime components and required function parameters specified at the operation level config into `ConfigBag`.

Once these changes are made, fluent builders will be able to do something like this:
```rust
// pseudo-code, possibly somewhere down the line of the `send_v2` method

let mut config_bag = /* create an empty config bag */;

// first put into the bag things from the service level config
self.handle.conf.configure(&mut config_bag);

// then put into the bag things from the operation level config
if let Some(config_override) = self.config_override {
    self.config_override.configure(&mut config_bag);
}
```
Note that if `ConfigLoader`, `SdkConfig`, and service configurations, including all of their builders, are implemented in terms of `ConfigBag`, implementing the `RuntimePlugin` trait may become unnecessary because the fields have already been stored in the bag. In that case, however, `ConfigBag`, which is held by `Arc<Handle>`, may need to be unshared so that it can be passed as `&mut` to `aws_smithy_runtime::client::orchestrator::invoke`.

#### How we plan to migrate today's runtime components to the world of the orchestrator

This part is a work in progress because we will learn more about the implementation details as we shift more and more runtime components to the world of the orchestrator. That said, here's our migration plan at a high level, using `DefaultResolver` as an example.

Currently, `DefaultResolver` and its user-specified parameters are wired together within `make_operation` on an operation input struct (such as `ListBucketsInput`).
```rust
// omitting unnecessary details

pub async fn make_operation(
    &self,
    _config: &crate::config::Config,
) -> std::result::Result<
    aws_smithy_http::operation::Operation<
        crate::operation::list_buckets::ListBuckets,
        aws_http::retry::AwsResponseRetryClassifier,
    >,
    aws_smithy_http::operation::error::BuildError,
> {
    let params_result = crate::endpoint::Params::builder()
        .set_region(_config.region.as_ref().map(|r| r.as_ref().to_owned()))
        .set_use_fips(_config.use_fips)
        .set_use_dual_stack(_config.use_dual_stack)
        // --snip--
        .set_accelerate(_config.accelerate)
        .build();

    let (endpoint_result, params) = match params_result {
        // `_config.endpoint_resolver` is a trait object whose underlying type
        // is `DefaultResolver`.
        Ok(params) => (
            _config.endpoint_resolver.resolve_endpoint(&params),
            Some(params),
        ),
        Err(e) => (Err(e), None),
    };

    // --snip--

    let mut properties = aws_smithy_http::property_bag::SharedPropertyBag::new();
    let body = aws_smithy_http::body::SdkBody::from("");
    let request = request.body(body).expect("should be valid request");
    let mut request = aws_smithy_http::operation::Request::from_parts(request, properties);
    request.properties_mut().insert(endpoint_result);
    if let Some(params) = params {
        request.properties_mut().insert(params);
    }

    // --snip--
}
```

The `endpoint_result` and `params` stored in `SharedPropertyBag` are later used by the [`MapRequestLayer` for `SmithyEndpointStage`](https://github.com/awslabs/smithy-rs/blob/main/rust-runtime/aws-smithy-http/src/endpoint/middleware.rs#L37-L83) to add to the request header as it is dispatched.

With the transition to the orchestrator world, the functionality performed by `make_operation` and `SmithyEndpointStage::apply` will be moved to `DefaultResolver`. Specifically, `DefaultResolver` will implement the `aws_smithy_runtime_api::client::orchestrator::EndpointResolver trait`:
```rust
impl aws_smithy_runtime_api::client::orchestrator::EndpointResolver for DefaultResolver {
    fn resolve_and_apply_endpoint(
        &self,
        request: &mut aws_smithy_runtime_api::client::orchestrator::HttpRequest,
        cfg: &aws_smithy_runtime_api::config_bag::ConfigBag,
    ) -> Result<(), aws_smithy_runtime_api::client::orchestrator::BoxError> {
        /*
         * Here, handles part of `make_operation` to yield `endpoint_result` and `params`.
         * We may need to assume that the entire `Params` is already available
         * in `cfg`, rather than extract every piece out of `cfg` necessary and
         * build a `Params` here.
         */

        /*
         * Later part of the method handles what SmithyEndpointStage does,
         * modifying the passed-in `request`.
         */
    }
}
```

Other runtime components have different requirements for their migration to the orchestrator world. A detailed roadmap for each of them is beyond the scope of this RFC.

However, the general refactoring pattern involves moving logic out of `make_operation` and the associated `Tower` layer, and placing it in a struct that implements the relevant trait defined in `aws_smithy_runtime_api::client::orchestrator`.

Changes checklist
-----------------

- [ ] Put client configuration into `ConfigBag` by either of the two
	- [ ] Implementing RuntimePlugin for service configs and service config builders
	- [ ] Updating `ConfigLoader`, `SdkConfig`, and service configs to be backed by `ConfigBag`
- [ ] Decide whether `aws_smithy_runtime_api::client::orchestrator::TraceProbe` should be supported when users can already set up `tracing::Subscriber` in the SDK
- [ ] Integrate the default implementation for `aws_smithy_runtime_api::client::orchestrator::RetryStrategy` with the orchestrator
- [ ] Integrate the default implementation for `aws_smithy_runtime_api::client::orchestrator::EndpointResolver` with the orchestrator
- [ ] Integrate the default implementation for `aws_smithy_runtime_api::client::orchestrator::Connection` with the orchestrator
- [ ] Integrate the default implementation for `aws_smithy_runtime_api::client::orchestrator::IdentityResolvers` with the orchestrator
- [ ] Integrate the default implementations for `aws_smithy_runtime_api::client::orchestrator::HttpAuthSchemes` and `aws_smithy_runtime_api::client::orchestrator::AuthOptionResolver` with the orchestrator

Appendix: Exposing `.runtime_plugin` through config types
---------------------------------------------------------
Alternatively, a `runtime_plugin` method could be added to `ConfigLoader`, `SdkConfig`, and service configs. The method would look like this:
```rust
pub fn runtime_plugin(
    mut self,
    runtime_plugin: impl RuntimePlugin + 'static,
) -> Self
{
    todo!()
}
```
However, this approach forces users to implement the `RuntimePlugin` trait for a given type, leading to the following issues:
- The `RuntimePlugin::configure` method takes a `&mut ConfigBag`, and it may not be immediately clear to users what they should put into or extract from the bag to implement the method. In contrast, methods like `endpoint_url` or `retry_config` are self-explanatory, and users are already familiar with them.
- With the `runtime_plugin` method, there could be two ways to configure the same runtime component. If a user specifies both `endpoint_url` and `runtime_plugin(/* a plugin for an endpoint */)` at the same time on `ConfigLoader`, it would be necessary to determine which one takes precedence.

For these reasons, this solution was dismissed as such.
