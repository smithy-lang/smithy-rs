# Smithy Orchestrator

> status: implemented
> applies-to: The smithy client

This RFC proposes a new process for constructing client requests and handling service responses. This new process is intended to:

- Improve the user experience by
- Simplifying several aspects of sending a request
- Adding more extension points to the request/response lifecycle
- Improve the maintainer experience by
- Making our SDK more similar in structure to other AWS SDKs
- Simplifying many aspects of the request/response lifecycle
- Making room for future changes

Additionally, functionality that the SDKs currently provide like retries, logging, and auth with be incorporated into this new process in such a way as to make it more configurable and understandable.

This RFC references but is not the source of truth on:

- Interceptors: To be described in depth in a future RFC.
- Runtime Plugins: To be described in depth in a future RFC.

## TLDR;

When a smithy client communicates with a smithy service, messages are handled by an "orchestrator." The orchestrator runs in two main phases:
1. Constructing configuration.
    - This process is user-configurable with "runtime plugins."
    - Configuration is stored in a typemap.
1. Transforming a client request into a server response.
    - This process is user-configurable with "interceptors."
    - Interceptors are functions that are run by "hooks" in the request/response lifecycle.

## Terminology

- **SDK Client**: A high-level abstraction allowing users to make requests to remote services.
- **Remote Service**: A remote API that a user wants to use. Communication with a remote service usually happens over HTTP. The remote service is usually, but not necessarily, an AWS service.
- **Operation**: A high-level abstraction representing an interaction between an *SDK Client and a *remote service*.
- **Input Message**: A modeled request passed into an *SDK client*. For example, S3’s `ListObjectsRequest`.
- **Transport Request Message**: A message that can be transmitted to a *remote service*. For example, an HTTP request.
- **Transport Response Message**: A message that can be received from a *remote service*. For example, an HTTP response.
- **Output Message**: A modeled response or exception returned to an *SDK client* caller. For example, S3’s `ListObjectsResponse` or `NoSuchBucketException`.
- **The request/response lifecycle**: The process by which an *SDK client* makes requests and receives responses from a *remote service*. This process is enacted and managed by the *orchestrator*.
- **Orchestrator**: The code within an *SDK client* that handles the process of making requests and receiving responses from *remote services*. The orchestrator is configurable by modifying the *runtime plugins* it's built from. The orchestrator is responsible for calling *interceptors* at the appropriate times in the *request/response lifecycle*.
- **Interceptor**/**Hook**: A generic extension point within the *orchestrator*. Supports "anything that someone should be able to do", NOT "anything anyone might want to do". These hooks are:
    - Either **read-only** or **read/write**.
    - Able to read and modify the **Input**, **Transport Request**, **Transport Response**, or **Output** messages.
- **Runtime Plugin**: Runtime plugins are similar to interceptors, but they act on configuration instead of requests and response. Both users and services may define runtime plugins. Smithy also defines several default runtime plugins used by most clients. See the F.A.Q. for a list of plugins with descriptions.
- **ConfigBag**: A `typemap` that's equivalent to [`http::Extensions`](https://docs.rs/http/latest/http/struct.Extensions.html). Used to store configuration for the orchestrator.

## The user experience if this RFC is implemented

For many users, the changes described by this RFC will be invisible. Making a request with an orchestrator-based SDK client looks very similar to the way requests were made pre-RFC:

```rust,ignore
let sdk_config = aws_config::load_from_env().await;
let client = aws_sdk_s3::Client::new(&sdk_config);
let res = client.get_object()
    .bucket("a-bucket")
    .key("a-file.txt")
    .send()
    .await?;

match res {
    Ok(res) => println!("success: {:?}"),
    Err(err) => eprintln!("failure: {:?}")
};
```

Users may further configure clients and operations with **runtime plugins**, and they can modify requests and responses with **interceptors**. We'll examine each of these concepts in the following sections.

### Service clients and operations are configured with runtime plugins

> The exact implementation of **runtime plugins** is left for another RFC. That other RFC will be linked here once it's written. To get an idea of what they may look like, see the *"Layered configuration, stored in type maps"* section of this RFC.

Runtime plugins construct and modify client configuration. Plugin initialization is the first step of sending a request, and plugins set in later steps can override the actions of earlier plugins. Plugin ordering is deterministic and non-customizable.

While AWS services define a default set of plugins, users may define their own plugins, and set them by calling the appropriate methods on a service's config, client, or operation. Plugins are specifically meant for constructing service and operation configuration. If a user wants to define behavior that should occur at specific points in the *request/response lifecycle*, then they should instead consider defining an *interceptor*.

### Requests and responses are modified by interceptors

Interceptors are similar to middlewares, in that they are functions that can read and modify request and response state. However, they are more restrictive than middlewares in that they can't modify the "control flow" of the request/response lifecycle. This is intentional. Interceptors can be registered on a client or operation, and the orchestrator is responsible for calling interceptors at the appropriate time. Users MUST NOT perform blocking IO within an interceptor. Interceptors are sync, and are not intended to perform large amounts of work. This makes them easier to reason about and use. Depending on when they are called, interceptors may read and modify *input messages*, *transport request messages*, *transport response messages*, and *output messages*. Additionally, all interceptors may write to a context object that is shared between all interceptors.

#### Currently supported hooks

1. **Read Before Execution *(Read-Only)***: Before anything happens. This is the first
   thing the SDK calls during operation execution.
1. **Modify Before Serialization *(Read/Write)***: Before the input message given by
   the customer is marshalled into a transport request message. Allows modifying the
   input message.
1. **Read Before Serialization *(Read-Only)***: The last thing the SDK calls before
   marshaling the input message into a transport message.
1. **Read After Serialization *(Read-Only)***: The first thing the SDK calls after marshaling the input message into a transport message.
1. *(Retry Loop)*
    1. **Modify Before Retry Loop *(Read/Write)***: The last thing the SDK calls before entering the retry look. Allows modifying the transport message.
    1. **Read Before Attempt *(Read-Only)***: The first thing the SDK calls “inside” of the retry loop.
    1. **Modify Before Signing *(Read/Write)***: Before the transport request message is signed. Allows modifying the transport message.
    1. **Read Before Signing *(Read-Only)***: The last thing the SDK calls before signing the transport request message.
    1. **Read After Signing (Read-Only)****: The first thing the SDK calls after signing the transport request message.
    1. **Modify Before Transmit *(Read/Write)***: Before the transport request message is sent to the service. Allows modifying the transport message.
    1. **Read Before Transmit *(Read-Only)***: The last thing the SDK calls before sending the transport request message.
    1. **Read After Transmit *(Read-Only)***: The last thing the SDK calls after receiving the transport response message.
    1. **Modify Before Deserialization *(Read/Write)***: Before the transport response message is unmarshaled. Allows modifying the transport response message.
    1. **Read Before Deserialization *(Read-Only)***: The last thing the SDK calls before unmarshalling the transport response message into an output message.
    1. **Read After Deserialization *(Read-Only)***: The last thing the SDK calls after unmarshaling the transport response message into an output message.
    1. **Modify Before Attempt Completion *(Read/Write)***: Before the retry loop ends. Allows modifying the unmarshaled response (output message or error).
    1. **Read After Attempt *(Read-Only)***: The last thing the SDK calls “inside” of the retry loop.
1. **Modify Before Execution Completion *(Read/Write)***: Before the execution ends. Allows modifying the unmarshaled response (output message or error).
1. **Read After Execution *(Read-Only)***: After everything has happened. This is the last thing the SDK calls during operation execution.

### Interceptor context

As mentioned above, interceptors may read/write a context object that is shared between all interceptors:

```rust,ignore
pub struct InterceptorContext<ModReq, TxReq, TxRes, ModRes> {
    // a.k.a. the input message
    modeled_request: ModReq,
    // a.k.a. the transport request message
    tx_request: Option<TxReq>,
    // a.k.a. the output message
    modeled_response: Option<ModRes>,
    // a.k.a. the transport response message
    tx_response: Option<TxRes>,
    // A type-keyed map
    properties: SharedPropertyBag,
}
```

The optional request and response types in the interceptor context can only be accessed by interceptors that are run after specific points in the *request/response lifecycle*. Rather than go into depth in this RFC, I leave that to a future "Interceptors RFC."

## How to implement this RFC

### Integrating with the orchestrator

Imagine we have some sort of request signer. This signer doesn't refer to any orchestrator types. All it needs is a `HeaderMap` along with two strings, and will return a signature in string form.

```rust,ignore
struct Signer;

impl Signer {
    fn sign(headers: &http::HeaderMap, signing_name: &str, signing_region: &str) -> String {
        todo!()
    }
}
```

Now imagine things from the orchestrator's point of view. It requires something that implements an `AuthOrchestrator` which will be responsible for resolving the correct auth
scheme, identity, and signer for an operation, as well as signing the request

```rust,ignore
pub trait AuthOrchestrator<Req>: Send + Sync + Debug {
    fn auth_request(&self, req: &mut Req, cfg: &ConfigBag) -> Result<(), BoxError>;
}

// And it calls that `AuthOrchestrator` like so:
fn invoke() {
    // code omitted for brevity

    // Get the request to be signed
    let tx_req_mut = ctx.tx_request_mut().expect("tx_request has been set");
    // Fetch the auth orchestrator from the bag
    let auth_orchestrator = cfg
        .get::<Box<dyn AuthOrchestrator<Req>>>()
        .ok_or("missing auth orchestrator")?;
    // Auth the request
    auth_orchestrator.auth_request(tx_req_mut, cfg)?;

    // code omitted for brevity
}
```

The specific implementation of the `AuthOrchestrator` is what brings these two things together:

```rust,ignore
struct Sigv4AuthOrchestrator;

impl AuthOrchestrator for Sigv4AuthOrchestrator {
    fn auth_request(&self, req: &mut http::Request<SdkBody>, cfg: &ConfigBag) -> Result<(), BoxError> {
        let signer = Signer;
        let signing_name = cfg.get::<SigningName>().ok_or(Error::MissingSigningName)?;
        let signing_region = cfg.get::<SigningRegion>().ok_or(Error::MissingSigningRegion)?;
        let headers = req.headers_mut();

        let signature = signer.sign(headers, signing_name, signing_region);
        match cfg.get::<SignatureLocation>() {
            Some(SignatureLocation::Query) => req.query.set("sig", signature),
            Some(SignatureLocation::Header) => req.headers_mut().insert("sig", signature),
            None => return Err(Error::MissingSignatureLocation),
        };

        Ok(())
    }
}
```

This intermediate code should be free from as much logic as possible. Whenever possible, we must maintain this encapsulation. Doing so will make the Orchestrator more flexible, maintainable, and understandable.

### Layered configuration, stored in type maps

> **Type map**: A data structure where stored values are keyed by their type. Hence, only one value can be stored for a given type.
>
> *See [typemap](https://docs.rs/typemap), [type-map](https://docs.rs/crate/type-map), [http::Extensions](https://docs.rs/http/latest/http/struct.Extensions.html), and [actix_http::Extensions](https://docs.rs/actix-http/latest/actix_http/struct.Extensions.html) for examples.*

```rust,ignore
 let conf: ConfigBag = aws_config::from_env()
    // Configuration can be common to all smithy clients
    .with(RetryConfig::builder().disable_retries().build())
    // Or, protocol-specific
    .with(HttpClient::builder().build())
    // Or, AWS-specific
    .with(Region::from("us-east-1"))
    // Or, service-specific
    .with(S3Config::builder().force_path_style(false).build())
    .await;

let client = aws_sdk_s3::Client::new(&conf);

client.list_buckets()
    .customize()
    // Configuration can be set on operations as well as clients
    .with(HttpConfig::builder().conn(some_other_conn).build())
    .send()
    .await;
```

Setting configuration that will not be used wastes memory and can make debugging more difficult. Therefore, configuration defaults are only set when they're relevant. For example, if a smithy service doesn't support HTTP, then no HTTP client will be set.

#### What is "layered" configuration?

Configuration has precedence. Configuration set on an operation will override configuration set on a client, and configuration set on a client will override default configuration. However, configuration with a higher precedence can also augment configuration with a lower precedence. For example:

```rust,ignore
let conf: ConfigBag = aws_config::from_env()
    .with(
        SomeConfig::builder()
            .option_a(1)
            .option_b(2)
            .option_c(3)
    )
    .build()
    .await;

let client = aws_sdk_s3::Client::new(&conf);

client.list_buckets()
    .customize()
    .with(
        SomeConfig::builder()
            .option_a(0)
            .option_b(Value::Inherit)
            .option_c(Value::Unset)
    )
    .build()
    .send()
    .await;
```

In the above example, when the `option_a`, `option_b`, `option_c`, values of `SomeConfig` are accessed, they'll return:

- `option_a`: `0`
- `option_b`: `2`
- `option_c`: No value

Config values are wrapped in a special enum called `Value` with three variants:

- `Value::Set`: A set value that will override values from lower layers.
- `Value::Unset`: An explicitly unset value that will override values from lower layers.
- `Value::Inherit`: An explicitly unset value that will inherit a value from a lower layer.

Builders are defined like this:

```rust,ignore
struct SomeBuilder {
    value: Value<T>,
}

impl struct SomeBuilder<T> {
    fn new() -> Self {
        // By default, config values inherit from lower-layer configs
        Self { value: Value::Inherit }
    }

    fn some_field(&mut self, value: impl Into<Value<T>>) -> &mut self {
        self.value = value.into();
        self
    }
}
```

Because of `impl Into<Value<T>>`, users don't need to reference the `Value` enum unless they want to "unset" a value.

#### Layer separation and precedence

Codegen defines default sets of interceptors and runtime plugins at various "levels":

1. AWS-wide defaults set by codegen.
1. Service-wide defaults set by codegen.
1. Operation-specific defaults set by codegen.

Likewise, users may mount their own interceptors and runtime plugins:

1. The AWS config level, e.g. `aws_types::Config`.
1. The service config level, e.g. `aws_sdk_s3::Config`.
1. The operation config level, e.g. `aws_sdk_s3::Client::get_object`.

Configuration is resolved in a fixed manner by reading the "lowest level" of config available, falling back to "higher levels" only when no value has been set. Therefore, at least 3 separate `ConfigBag`s are necessary, and user configuration has precedence over codegen-defined default configuration. With that in mind, resolution of configuration would look like this:

1. Check user-set operation config.
1. Check codegen-defined operation config.
1. Check user-set service config.
1. Check codegen-defined service config.
1. Check user-set AWS config.
1. Check codegen-defined AWS config.

### The `aws-smithy-orchestrator` crate

*I've omitted some of the error conversion to shorten this example and make it easier to understand. The real version will be messier.*

```rust,ignore
/// `In`: The input message e.g. `ListObjectsRequest`
/// `Req`: The transport request message e.g. `http::Request<SmithyBody>`
/// `Res`: The transport response message e.g. `http::Response<SmithyBody>`
/// `Out`: The output message. A `Result` containing either:
///     - The 'success' output message e.g. `ListObjectsResponse`
///     - The 'failure' output message e.g. `NoSuchBucketException`
pub async fn invoke<In, Req, Res, T>(
    input: In,
    interceptors: &mut Interceptors<In, Req, Res, Result<T, BoxError>>,
    runtime_plugins: &RuntimePlugins,
    cfg: &mut ConfigBag,
) -> Result<T, BoxError>
    where
        // The input must be Clone in case of retries
        In: Clone + 'static,
        Req: 'static,
        Res: 'static,
        T: 'static,
{
    let mut ctx: InterceptorContext<In, Req, Res, Result<T, BoxError>> =
        InterceptorContext::new(input);

    runtime_plugins.apply_client_configuration(cfg)?;
    interceptors.client_read_before_execution(&ctx, cfg)?;

    runtime_plugins.apply_operation_configuration(cfg)?;
    interceptors.operation_read_before_execution(&ctx, cfg)?;

    interceptors.read_before_serialization(&ctx, cfg)?;
    interceptors.modify_before_serialization(&mut ctx, cfg)?;

    let request_serializer = cfg
        .get::<Box<dyn RequestSerializer<In, Req>>>()
        .ok_or("missing serializer")?;
    let req = request_serializer.serialize_request(ctx.modeled_request_mut(), cfg)?;
    ctx.set_tx_request(req);

    interceptors.read_after_serialization(&ctx, cfg)?;
    interceptors.modify_before_retry_loop(&mut ctx, cfg)?;

    loop {
        make_an_attempt(&mut ctx, cfg, interceptors).await?;
        interceptors.read_after_attempt(&ctx, cfg)?;
        interceptors.modify_before_attempt_completion(&mut ctx, cfg)?;

        let retry_strategy = cfg
            .get::<Box<dyn RetryStrategy<Result<T, BoxError>>>>()
            .ok_or("missing retry strategy")?;
        let mod_res = ctx
            .modeled_response()
            .expect("it's set during 'make_an_attempt'");
        if retry_strategy.should_retry(mod_res, cfg)? {
            continue;
        }

        interceptors.modify_before_completion(&mut ctx, cfg)?;
        let trace_probe = cfg
            .get::<Box<dyn TraceProbe>>()
            .ok_or("missing trace probes")?;
        trace_probe.dispatch_events(cfg);
        interceptors.read_after_execution(&ctx, cfg)?;

        break;
    }

    let (modeled_response, _) = ctx.into_responses()?;
    modeled_response
}

// Making an HTTP request can fail for several reasons, but we still need to
// call lifecycle events when that happens. Therefore, we define this
// `make_an_attempt` function to make error handling simpler.
async fn make_an_attempt<In, Req, Res, T>(
    ctx: &mut InterceptorContext<In, Req, Res, Result<T, BoxError>>,
    cfg: &mut ConfigBag,
    interceptors: &mut Interceptors<In, Req, Res, Result<T, BoxError>>,
) -> Result<(), BoxError>
    where
        In: Clone + 'static,
        Req: 'static,
        Res: 'static,
        T: 'static,
{
    interceptors.read_before_attempt(ctx, cfg)?;

    let tx_req_mut = ctx.tx_request_mut().expect("tx_request has been set");
    let endpoint_orchestrator = cfg
        .get::<Box<dyn EndpointOrchestrator<Req>>>()
        .ok_or("missing endpoint orchestrator")?;
    endpoint_orchestrator.resolve_and_apply_endpoint(tx_req_mut, cfg)?;

    interceptors.modify_before_signing(ctx, cfg)?;
    interceptors.read_before_signing(ctx, cfg)?;

    let tx_req_mut = ctx.tx_request_mut().expect("tx_request has been set");
    let auth_orchestrator = cfg
        .get::<Box<dyn AuthOrchestrator<Req>>>()
        .ok_or("missing auth orchestrator")?;
    auth_orchestrator.auth_request(tx_req_mut, cfg)?;

    interceptors.read_after_signing(ctx, cfg)?;
    interceptors.modify_before_transmit(ctx, cfg)?;
    interceptors.read_before_transmit(ctx, cfg)?;

    // The connection consumes the request but we need to keep a copy of it
    // within the interceptor context, so we clone it here.
    let res = {
        let tx_req = ctx.tx_request_mut().expect("tx_request has been set");
        let connection = cfg
            .get::<Box<dyn Connection<Req, Res>>>()
            .ok_or("missing connector")?;
        connection.call(tx_req, cfg).await?
    };
    ctx.set_tx_response(res);

    interceptors.read_after_transmit(ctx, cfg)?;
    interceptors.modify_before_deserialization(ctx, cfg)?;
    interceptors.read_before_deserialization(ctx, cfg)?;
    let tx_res = ctx.tx_response_mut().expect("tx_response has been set");
    let response_deserializer = cfg
        .get::<Box<dyn ResponseDeserializer<Res, Result<T, BoxError>>>>()
        .ok_or("missing response deserializer")?;
    let res = response_deserializer.deserialize_response(tx_res, cfg)?;
    ctx.set_modeled_response(res);

    interceptors.read_after_deserialization(ctx, cfg)?;

    Ok(())
}
```

#### Traits

At various points in the execution of `invoke`, trait objects are fetched from the `ConfigBag`. These are preliminary definitions of those traits:

```rust,ignore
pub trait TraceProbe: Send + Sync + Debug {
    fn dispatch_events(&self, cfg: &ConfigBag) -> BoxFallibleFut<()>;
}

pub trait RequestSerializer<In, TxReq>: Send + Sync + Debug {
    fn serialize_request(&self, req: &mut In, cfg: &ConfigBag) -> Result<TxReq, BoxError>;
}

pub trait ResponseDeserializer<TxRes, Out>: Send + Sync + Debug {
    fn deserialize_response(&self, res: &mut TxRes, cfg: &ConfigBag) -> Result<Out, BoxError>;
}

pub trait Connection<TxReq, TxRes>: Send + Sync + Debug {
    fn call(&self, req: &mut TxReq, cfg: &ConfigBag) -> BoxFallibleFut<TxRes>;
}

pub trait RetryStrategy<Out>: Send + Sync + Debug {
    fn should_retry(&self, res: &Out, cfg: &ConfigBag) -> Result<bool, BoxError>;
}

pub trait AuthOrchestrator<Req>: Send + Sync + Debug {
    fn auth_request(&self, req: &mut Req, cfg: &ConfigBag) -> Result<(), BoxError>;
}

pub trait EndpointOrchestrator<Req>: Send + Sync + Debug {
    fn resolve_and_apply_endpoint(&self, req: &mut Req, cfg: &ConfigBag) -> Result<(), BoxError>;
    fn resolve_auth_schemes(&self) -> Result<Vec<String>, BoxError>;
}
```

## F.A.Q.

- The orchestrator is a large and complex feature, with many moving parts. How can we ensure that multiple people can contribute in parallel?
  - By defining the entire orchestrator and agreeing on its structure, we can then move on to working on individual runtime plugins and interceptors.
- What is the precedence of interceptors?
    - The precedence of interceptors is as follows:
        - Interceptors registered via Smithy default plugins.
        - *(AWS Services only)* Interceptors registered via AWS default plugins.
        - Interceptors registered via service-customization plugins.
        - Interceptors registered via client-level plugins.
        - Interceptors registered via client-level configuration.
        - Interceptors registered via operation-level plugins.
        - Interceptors registered via operation-level configuration.
- What runtime plugins will be defined in `smithy-rs`?
    - `RetryStrategy`: Configures how requests are retried.
    - `TraceProbes`: Configures locations to which SDK metrics are published.
    - `EndpointProviders`: Configures which hostname an SDK will call when making a request.
    - `HTTPClients`: Configures how remote services are called.
    - `IdentityProviders`: Configures how customers identify themselves to remote services.
    - `HTTPAuthSchemes` & `AuthSchemeResolvers`: Configures how customers authenticate themselves to remote services.
    - `Checksum Algorithms`: Configures how an SDK calculates request and response checksums.

## Changes checklist

- [x] Create a new `aws-smithy-runtime` crate.
  - Add orchestrator implementation
  - Define the orchestrator/runtime plugin interface traits
    - `TraceProbe`
    - `RequestSerializer<In, TxReq>`
    - `ResponseDeserializer<TxRes, Out>`
    - `Connection<TxReq, TxRes>`
    - `RetryStrategy<Out>`
    - `AuthOrchestrator<Req>`
    - `EndpointOrchestrator<Req>`
- [x] Create a new `aws-smithy-runtime-api` crate.
  - Add `ConfigBag` module
  - Add `retries` module
    - Add `rate_limiting` sub-module
  - Add `interceptors` module
    - `Interceptor` trait
    - `InterceptorContext` impl
  - Add `runtime_plugins` module
- [x] Create a new integration test that ensures the orchestrator works.
