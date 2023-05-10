Configuring SDK client for the orchestrator
===========================================

This document describes how to configure an SDK client for the orchestrator, with a focus on the following:
- Support for runtime components required by the orchestrator
- Support for operation-level configuration

As described in [RFC 34](https://github.com/awslabs/smithy-rs/blob/main/design/src/rfcs/rfc0034_smithy_orchestrator.md), the orchestrator prescribes configured components for an SDK to handle messages between the client and a service. They include:
- `RetryStrategy`: Configures how requests are retried.
- `TraceProbes`: Configures locations to which SDK metrics are published.
- `EndpointProviders`: Configures which hostname an SDK will call when making a request.
- `HTTPClients`: Configures how remote services are called.
- `IdentityProviders`: Configures how users identify themselves to remote services.
- `HTTPAuthSchemes` & `AuthSchemeResolver`s: Configures how users authenticate themselves to remote services.
- `Checksum Algorithms`: Configures how an SDK calculates request and response checksums.
- `Interceptors`: Configures specific stages of the request execution pipeline.

From an interface perspective, the aim of client configuration for the SDK is to provide public APIs for customizing these components as needed. From an implementation perspective, the goal of client configuration is to store the components in a typed configuration map for later use by the orchestrator during execution.

Configuring the client for the orchestrator will simplify support for operation-level configuration. There are use cases where configuring a client for a single operation invocation is necessary ([example](https://github.com/awslabs/aws-sdk-rust/issues/696)). At the time of writing, this feature is not yet supported, but operation-level configuration will address that limitation.

Please note that this document does not cover the configuration of the generic client in the orchestrator. That will be addressed in a separate design document.

Terminology
-----------
- Component: An interface coupled with its default implementations to enable an SDK client functionality.
- Fluent Client: A code generated Client that has methods for each service operation on it. A fluent builder is generated alongside it to make construction easier.
- Generic Client: A `aws_smithy_client::Client<C, M, R>` struct that is responsible for gluing together the connector, middleware, and retry policy. The concept still applies, but the struct will cease to exist with the introduction of the orchestrator.
- Operation: A high-level abstraction representing an interaction between an SDK Client and a remote service.
- Orchestrator: The code within an SDK client that handles the process of making requests and receiving responses from remote services.
- Remote Service: A remote API that a user wants to use. Communication with a remote service usually happens over HTTP. The remote service is usually, but not necessarily, an AWS service.
- SDK Client: A client generated for the AWS SDK, allowing users to make requests to remote services.

The user experience
-------------------

### Configuring runtime components required by the orchestrator

`ConfigLoader`, `SdkConfig`, and service configs allow users to configure the necessary runtime components for today's [`Tower`-based infrastructure](https://github.com/awslabs/smithy-rs/blob/35f2f27a8380a1310c264a386e162cd9f2180137/rust-runtime/aws-smithy-client/src/lib.rs#L155-L245). The degree to which these components can be configured in the orchestrator will remain largely unchanged.

The following table shows for each runtime component (the left column), what method on `ConfigLoader`, `sdk_config::Builder`, and service config builder (e.g. `aws_sdk_s3::config::Builder`) are currently available (the middle column) and what new method will be available on those types as proposed by the design (the right column).

| Runtime component | Today's builder method | Proposed builder method |
| :-: | --- | --- |
| RetryStrategy | `.retry_config` | No change |
| TraceProbes | None | No plan to implement before general availability (GA) |
| EndpointResolver | `.endpoint_url` | No change |
| HTTPClients | `.http_connector` | No change |
| IdentityProviders | `.credentials_provider` | `.credentials_provider` + `.token_provider` |
| HTTPAuthSchemes & AuthSchemeResolvers | None | Not configurable on SDK client |
| Checksum Algorithms | Not configurable on SDK client | No change |
| Interceptors | None | `.interceptor(&self, impl Interceptor + 'static)` |

If components can be configured today, their builder methods continue to exist in the orchestrator. There are no plans to support `TraceProbes` prior to GA because users can still set up `tracing::Subscriber` to specify where metrics are published. For `IdentityProviders`, in addition to `.credentials_provider`, we will introduce `.token_provider` as part of [this PR](https://github.com/awslabs/smithy-rs/pull/2627). `HTTPAuthSchemes`, `AuthSchemeResolver`s, and `Checksum Algorithms` will only be configurable on the generic client, but not on the SDK client. Finally, customers will be able to configure interceptors to inject logic into specific stages of the request execution pipeline, which will be enabled by [this PR](https://github.com/awslabs/smithy-rs/pull/2669).

### Operation-level configuration

Currently, users are able to customize runtime configuration at multiple levels. `SdkConfig` is used to configure settings for all services, while a service config (e.g., `aws_sdk_s3::config::Config`) is used to configure settings for a specific service client. However, there has been no support for configuring settings for a single operation invocation.

With this design, users will be able to go one step further and override configuration for a single operation invocation:
```rust
let sdk_config = aws_config::from_env().load().await;
let s3_client = aws_sdk_s3::client::Client::new(&sdk_config);
s3_client.create_bucket()
    .bucket(bucket_name)
    .config_override(aws_sdk_s3::config::builder().region("us-west-1"))
    .send()
    .await;
```

This can be achieved through the `.config_override` method, which is added to fluent builders, such as `fluent_builders::CreateBucket`. This method sets the `us-west-1` region to override any region setting specified in the service level config. The operation level config takes the highest precedence, followed by the service level config, and then the AWS level config. Here is an example of how a config is overwritten by another config:
```
Config A     overridden by    Config B          ==        Config C
field_1: None,                field_1: Some(v2),          field_1: Some(v2),
field_2: Some(v1),            field_2: Some(v2),          field_2: Some(v2),
field_3: Some(v1),            field_3: None,              field_3: Some(v1),
```

The `config_override` method takes a service config builder instead of a service config, allowing `None` values to be used for fields, so as not to override settings at a lower-precedence configuration.

The main benefit of this approach is simplicity for users. The only change required is to call the `config_override` method on an operation input fluent builder. If users do not wish to specify the operation level config, their workflow will remain unaffected.

How the design has been implemented
-----------------------------------
As stated, the goal of client configuration implementation is to store the components in a typed configuration map for `aws_smithy_runtime::client::orchestrator::invoke` for later use during execution.

This section covers three parts:
- [Where we store operation-level config](#where-we-store-operation-level-config)
- [How we put runtime components and required function parameters into a type map](#how-we-put-runtime-components-and-required-function-parameters-into-a-type-map)
- [How we have been migrating today's runtime components to the orchestrator](#how-we-have-been-migrating-todays-runtime-components-to-the-orchestrator)

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

<img width="1367" alt="client config at different levels" src="https://github.com/awslabs/smithy-rs/assets/15333866/62674ba1-f30e-4435-bdf9-61fa90a74991">

A fluent builder `ListBucketsFluentBuilder` has access to the service config via `self.handle.conf` and to the operation-level config, if any, via `self.config_override`. To configure a client, the appropriate components need to be created from the fields of those configs and stored in a typed map, `ConfigBag`.

That can be accomplished in two steps:
1. When the fluent builder's [send_orchestrate](https://github.com/awslabs/smithy-rs/blob/a37b7382c14461709ec09b3a5a7c7fc6819e6173/codegen-client/src/main/kotlin/software/amazon/smithy/rust/codegen/client/smithy/generators/client/FluentClientGenerator.kt#L367-L398) method is called, it creates `RuntimePlugins` and then calls `aws_smithy_runtime::client::orchestrator::invoke` with it.
2. The `invoke` method calls `RuntimePlugin::configure` for each of the passed-in runtime plugins to store the components in the `ConfigBag`

For 1., the `send_orchestrate` method looks as follows, up to a point where it calls `invoke`:
```rust
let mut runtime_plugins =
    aws_smithy_runtime_api::client::runtime_plugin::RuntimePlugins::new()
        .with_client_plugin(crate::config::ServiceRuntimePlugin::new(
            self.handle.clone(),
        ));
if let Some(config_override) = self.config_override {
    runtime_plugins = runtime_plugins.with_operation_plugin(config_override);
}
runtime_plugins = runtime_plugins.with_operation_plugin(
    crate::operation::<operation_module>::<OperationName>::new(),
);

// --snip--

let input = self
    .inner
    .build()
    .map_err(aws_smithy_http::result::SdkError::construction_failure)?;
let input = aws_smithy_runtime_api::type_erasure::TypedBox::new(input).erase();
let output = aws_smithy_runtime::client::orchestrator::invoke(input, &runtime_plugins)

// --snip--
```
For the client level config, `ServiceRuntimePlugin` is created with the service config (`self.handle.conf`) and added as a client plugin. Similarly, for the operation level config, two runtime plugins are registered as operation plugins, one of which uses the config builder passed-in via `.config_override` earlier.

Adding plugins at this point will not do anything, but it sets things up so that when the `configure` method is called on a plugin, it creates components and stores them in `ConfigBag`. The following code snippet for `ServiceRuntimePlugin` shows that it creates components such as `EndpointResolver` and `IdentityResolver` and stores them in the `ConfigBag`. It also registers interceptors that will be invoked at specific stages of the request execution pipeline during `invoke`.
```rust
impl aws_smithy_runtime_api::client::runtime_plugin::RuntimePlugin for ServiceRuntimePlugin {
    fn configure(
        &self,
        cfg: &mut aws_smithy_runtime_api::config_bag::ConfigBag,
        _interceptors: &mut aws_smithy_runtime_api::client::interceptors::Interceptors,
    ) -> Result<(), aws_smithy_runtime_api::client::runtime_plugin::BoxError> {
        use aws_smithy_runtime_api::client::orchestrator::ConfigBagAccessors;

        // --snip--

        let http_auth_schemes = aws_smithy_runtime_api::client::auth::HttpAuthSchemes::builder()
            .auth_scheme(
                aws_runtime::auth::sigv4::SCHEME_ID,
                aws_runtime::auth::sigv4::SigV4HttpAuthScheme::new(),
            )
            .build();
        cfg.set_http_auth_schemes(http_auth_schemes);

        // Set an empty auth option resolver to be overridden by operations that need auth.
        cfg.set_auth_option_resolver(
            aws_smithy_runtime_api::client::auth::option_resolver::StaticAuthOptionResolver::new(
                Vec::new(),
            ),
        );

        let endpoint_resolver =
            aws_smithy_runtime::client::orchestrator::endpoints::DefaultEndpointResolver::<
                crate::endpoint::Params,
            >::new(aws_smithy_http::endpoint::SharedEndpointResolver::from(
                self.handle.conf.endpoint_resolver(),
            ));
        cfg.set_endpoint_resolver(endpoint_resolver);

        // --snip--

        _interceptors.register_client_interceptor(std::sync::Arc::new(
            aws_runtime::user_agent::UserAgentInterceptor::new(),
        ) as _);
        _interceptors.register_client_interceptor(std::sync::Arc::new(
            aws_runtime::invocation_id::InvocationIdInterceptor::new(),
        ) as _);
        _interceptors.register_client_interceptor(std::sync::Arc::new(
            aws_runtime::recursion_detection::RecursionDetectionInterceptor::new(),
        ) as _);

        // --snip--

        cfg.set_identity_resolvers(
            aws_smithy_runtime_api::client::identity::IdentityResolvers::builder()
                .identity_resolver(
                    aws_runtime::auth::sigv4::SCHEME_ID,
                    aws_runtime::identity::credentials::CredentialsIdentityResolver::new(
                        self.handle.conf.credentials_cache(),
                    ),
                )
                .build(),
        );
        Ok(())
    }
}
```

For 2., the first thing `invoke` does is to [apply client runtime plugins and operation plugins](https://github.com/awslabs/smithy-rs/blob/main/rust-runtime/aws-smithy-runtime/src/client/orchestrator.rs#L75-L80). That will actually execute the `configure` method as shown above to set up `ConfigBag` properly. It is crucial to apply client runtime plugins first and then operation runtime plugins because what's applied later will end up in a more recent layer in `ConfigBag` and the operation-level config should take precedence over the client-level config.

#### How we have been migrating today's runtime components to the orchestrator

We have been porting the existing components designed to run in the Tower-based infrastructure over to the orchestrator. The following PRs demonstrate examples:
- [Add SigV4 support to the orchestrator](https://github.com/awslabs/smithy-rs/pull/2533)
- [Implement the UserAgentInterceptor for the SDK](https://github.com/awslabs/smithy-rs/pull/2550)
- [Add DefaultEndpointResolver to the orchestrator](https://github.com/awslabs/smithy-rs/pull/2577)

While each PR tackles different part of runtime components, some of them share a common porting strategy, which involves moving logic out of `make_operation` on an operation input struct and logic out of the associated Tower layer, and placing them in a struct that implements the relevant trait defined in [aws_smithy_runtime_api::client::orchestrator](https://github.com/awslabs/smithy-rs/blob/2b165037fd785ce122c993c1a59d3c8d5a3e522c/rust-runtime/aws-smithy-runtime/src/client/orchestrator/endpoints.rs#L81-L130).

Taking the third PR, for instance, `DefaultResolver` today and its user-specified parameters are wired together within `make_operation` on an operation input struct (such as `ListBucketsInput`).
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

With the transition to the orchestrator, the functionality performed by `make_operation` and `SmithyEndpointStage::apply` has been ported to [`DefaultEndpointResolver`](https://github.com/awslabs/smithy-rs/blob/2b165037fd785ce122c993c1a59d3c8d5a3e522c/rust-runtime/aws-smithy-runtime/src/client/orchestrator/endpoints.rs#L81-L130) which implements the `aws_smithy_runtime_api::client::orchestrator::EndpointResolver trait`.
