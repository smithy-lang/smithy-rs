# Smithy Orchestrator

> status: RFC
> applies-to: The smithy client

This RFC proposes a new process for constructing client requests and handling service responses. This new process is intended to:

- Improve the user experience by
  - Simplifying several aspects of sending a request
  - Adding more extension points to the request/response lifecycle
- Improve the maintainer experience by
  - Making our SDK more similar in structure to other AWS SDKs
  - Simplifying many aspects of the request/response lifecycle
  - Making room for future changes

Additionally, functionality that the SDKs currently provide like retries, logging, and auth with be incorporated into this new
process in such a way as to make it more configurable and understandable.

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
- **Runtime Plugin**: A runtime plugin defines instructions for how an *SDK client* is configured to use the components below:
	- `RetryStrategy`: Defines how requests are retried.
	- `TraceProbes`: Defines locations to which SDK metrics are published.
	- `EndpointProviders`: Defines which hostname an SDK will call when making a request.
	- `HTTPClients`: Defines how remote services are called.
	- `IdentityProviders`: Defines how customers identify themselves to remote services.
	- `HTTPAuthSchemes` & `AuthSchemeResolvers`: Defines how customers authenticate themselves to remote services.
	- `Checksum Algorithms`: Defines how an SDK calculates request and response checksums.

## The user experience if this RFC is implemented

For many users, the changes described by this RFC will be invisible. Making a request with an orchestrator-based SDK client looks very similar to the way requests are made pre-RFC:

```rust
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

However, if a user wishes to configure clients and operations, they may do so by setting **runtime plugins**.

### Configuring clients and operations with runtime plugins

Runtime plugins construct and modify client configuration. Plugin initialization is the first step of sending a request, and plugins set in later steps can override the actions of earlier plugins. Plugin ordering is deterministic and non-customizable.

While AWS services define a default set of plugins, users may define their own plugins, and set them by calling the appropriate methods on a service's config, client, or operation:

```rust
let service_config = aws_sdk_s3::Config::builder()
	// Multiple interceptors may be added
	.with_interceptor(CustomInterceptor::new())
	// Multiple trace probes may be added
	.with_trace_probe(CustomTraceProbe::new())
	// Multiple identity providers may be added
	.with_identity_provider(CustomIdProvider::new())
	// Multiple checksum algorithms may be added, but if a checksum algorithm's
	// ID matches the ID of an existing algorithm, then the new algorithm will
	// overwrite the old one.
	.with_checksum_algorithm(CustomChecksumAlgorithm::new())
	// Only one retry strategy may be set. Setting a new one will replace the
	// old one.
	.with_retry_strategy(CustomRetryStrategy::new())
	// Services also define protocol-specific configuration. As an example, here
	// are some HTTP client setting.
	.with_http_client(CustomHttpClient::new())
	//
	.with_http_auth_scheme(CustomHttpAuthScheme::new())
	.build();

// TODO HTTP client configuration is called out as separate in the internal design docs. How should it be configured?

// Plugins can be set on clients.
let client = aws_sdk_s3::Client::builder()
	.config(&sdk_config)
	.with_plugin(OpenTelemetryPlugin::new())
.build();

// Plugins can be set on operations by using the `customize` method.
let res = client.get_object()
	.bucket("a-bucket")
	.key("a-file.txt")
	.customize(|mut op| {
		// Check to see if the `SpecialOperationLogger` was already set.
		// If no, then set it and add a special header before returning
		// the modified operation.
		if !op.plugins().contains(SpecialOperationLogger::id()) {
			// set_plugin is just like `with_plugin` except it takes a
			// `&mut self` so that we don't need to re-assign `op`.
			op.set_plugin(SpecialOperationLogger::new());
			op.headers_mut().insert("Is-Special", "yes");
		}

		op
	})
	.send()
	.await?;
```

Plugins are specifically meant for constructing service and operation configuration. If a user wants to define behavior that should occur at specific points in the *request/response lifecycle*, then they should instead consider defining an *interceptor*.

### Configuring interceptors

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

```rust
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

### The `aws-smithy-orchestrator` crate

*I've omitted some of the error conversion to shorten this example and make it easier to understand. The real version will be messier.*

```rust
pub struct SmithyConfig<In, T, E, Cfg, Ep, To> {
    pub interceptors: Interceptors<
	    In,
		http::Request<SdkBody>,
		http::Response<SdkBody>,
		Result<T, E>
    >,
    pub response_deserializer: DeserializeResponse<T, E>,
    pub token_bucket: Arc<dyn TokenBucket>,
    pub request_serializer: SerializeRequest<In>,
    pub endpoint_resolver: Arc<dyn ResolveEndpoint<Ep>>,
    // These still need to be generic either because they're service-specific
    // or not object safe.
    pub service_config: Cfg,
    pub retry_classifier: ClassifyRetry<SdkSuccess<T>, SdkError<E>>,
    pub endpoint_parameters: Ep,
}

// These two macros help shorten the `make_an_attempt` function and make it
// more readable
macro_rules! bail_unless_ok {
    ($ctx: ident, $inp: expr) => {
        match $inp {
            Ok(ok) => ok,
            Err(err) => {
                $ctx.set_modeled_response(Err(err));
                return;
            }
        }
    };
}

macro_rules! bail_if_err {
    ($ctx: ident, $inp: expr) => {
        if let Err(err) = $inp {
            $ctx.set_modeled_response(Err(err));
            return;
        }
    };
}

/// `In`: The input message e.g. `ListObjectsRequest`
/// `T`: The 'success' output message e.g. `ListObjectsResponse`
/// `E`: The 'failure' output message e.g. `NoSuchBucketException`
/// `Cfg`: The service config
/// `Ep`: An endpoint parameters struct
/// `To`: A token from a token bucket
pub async fn invoke<In, T, E, Cfg, Ep, To>(
    input: In,
    cfg: &Cfg,
    mut connection: Connector,
) -> Result<SdkSuccess<T>, SdkError<E>>
where
	// The input must be Clone in case of retries
    In: Clone,
    // Because the invoke function is not service-specific, and must be able to
    // consume service-specific configuration, it needs to convert that config
    // into a more generic form. We must also clone it before applying runtime
    // plugins.
    Cfg: Clone + ApplyPlugins + Into<SmithyConfig>,
    // Errors that occur during the request/response lifecycle must be classifiable
    // by the request retryer and the token bucket.
    E: ProvideErrorKind,
{
	// Create a new interceptor context.
	// This will be passed to each interceptor in turn.
    let mut ctx = InterceptorContext::new(req);
	// We clone the config and apply all registered plugins to it, before converting
	// it into a type we can use.
	let cfg = cfg.clone().apply_plugins().into();

    interceptors.read_before_execution(&ctx)?;
    interceptors.modify_before_serialization(&mut ctx)?;
    interceptors.read_before_serialization(&ctx)?;

	// We clone the input, serialize it into ""-form,
	// and store it in the interceptor context.
    let mod_req = ctx.modeled_request().clone();
    let req = request_serializer(mod_req)?;
    ctx.set_tx_request(req);

    interceptors.read_after_serialization(&ctx)?;
    interceptors.modify_before_retry_loop(&mut ctx)?;

	// Making an HTTP request can fail for several reasons, but we still need to
	// call lifecycle events when that happens. Therefore, we define this
	// `make_an_attempt` function to make error handling simpler.
    async fn make_an_attempt<Ep, In, T, E>(
        ctx: &mut InterceptorContext<
	        In,
	        http::Request<SdkBody>,
	        http::Response<SdkBody>,
	        Result<SdkSuccess<T>, SdkError<E>>
        >,
        interceptors: &mut Interceptors<
		    In,
			http::Request<SdkBody>,
			http::Response<SdkBody>,
			Result<T, E>
        >,
        endpoint_resolver: &dyn ResolveEndpoint<Ep>,
        endpoint_parameters: &Ep,
        connection: &mut Connector,
        response_deserializer: &DeserializeResponse<T, E>,
    ) where
        In: Clone,
    {
        interceptors.read_before_attempt(ctx);
        let auth_schemes = bail_unless_ok!(
	        ctx,
	        resolve_auth_schemes(endpoint_resolver, endpoint_parameters)
		);
        let signer = get_signer_for_first_supported_auth_scheme(&auth_schemes);
        let identity = bail_unless_ok!(
	        ctx,
	        auth_scheme.resolve_identity()
		);
        bail_if_err!(
            ctx,
            resolve_and_apply_endpoint(ctx, endpoint_resolver, &endpoint_parameters)
        );

        bail_if_err!(ctx, interceptors.modify_before_signing(ctx)  );
        // 7.h
        bail_if_err!(ctx, interceptors.read_before_signing(ctx));
        {
            let (tx_req_mut, props) = ctx.tx_request_mut()
	            .expect("tx_request has been set");
            if let Err(err) = signer(tx_req_mut, &props) {
                drop(props);
                ctx.set_modeled_response(Err(err.into()));
                return;
            }
        }
        bail_if_err!(ctx, interceptors.read_after_signing(ctx));
        bail_if_err!(ctx, interceptors.modify_before_transmit(ctx));
        bail_if_err!(ctx, interceptors.read_before_transmit(ctx));

		// The connection consumes the request but we need to keep a copy of it
		// within the interceptor context, so we clone it here.
        let tx_req = try_clone_http_request(
	        ctx.tx_request().expect("tx_request has been set")
	    ).expect("tx_request is cloneable");
        let res = bail_unless_ok!(ctx, connection(tx_req).await);
        ctx.set_tx_response(res);

        bail_if_err!(ctx, interceptors.read_after_transmit(ctx));
        bail_if_err!(ctx, interceptors.modify_before_deserialization(ctx));
        bail_if_err!(ctx, interceptors.read_before_deserialization(ctx));

        let (tx_res, _) = ctx.tx_response_mut().expect("tx_response has been set");
        let res = response_deserializer(tx_res);
        ctx.set_modeled_response(res);

        bail_if_err!(ctx, interceptors.read_after_deserialization(&ctx));
    }

    let mut attempt = 0;
    loop {
        if attempt > config.retry.max_attempts() {
            break;
        }

		// Acquire initial request token. Some retry/quota strategies don't require a
	    // token for the initial request. We must always ask for the token,
	    // but taking it may not affect the number of tokens in the bucket.
	    let mut token = Some(token_bucket.try_acquire(None).map_err(|err| {
	        SdkError::dispatch_failure(ConnectorError::other(
	            Box::new(err),
	            Some(ErrorKind::ClientError),
	        ))
	    })?);

        // For attempt other than the first, we need to clear out data set in previous
        // attempts.
        if attempt > 0 {
            ctx.reset();
        }

        make_an_attempt(
            &mut ctx,
            &mut interceptors,
            &signer,
            endpoint_resolver.as_ref(),
            &endpoint_parameters,
            &mut connection,
            &response_deserializer,
        )
        .await;

        interceptors.read_after_attempt(&ctx)?;
        interceptors.modify_before_attempt_completion(&mut ctx)?;

		// Figure out if the last attempt succeeded or failed
        let retry_kind: RetryKind = retry_classifier(
            ctx.modeled_response().expect("modeled_response has been set").as_ref(),
        );
        match retry_kind {
            RetryKind::Explicit(_duration) => {
                attempt += 1;
                token.take().unwrap().forget();
                continue;
            }
            RetryKind::Error(_err) => {
                attempt += 1;
                token.take().unwrap().forget();
                continue;
            }
            RetryKind::UnretryableFailure => {
                token.take().unwrap().forget();
            }
            RetryKind::Unnecessary => {
                token.take().unwrap().release();
                if attempt == 0 {
                    // Some token buckets refill if a request succeeds with retrying
                    token_bucket.refill_on_instant_success();
				}
            }
            _unhandled => unreachable!("encountered unhandled RetryKind {_unhandled:?}"),
        }

        interceptors.modify_before_completion(&mut ctx)?;
        // Dispatch logging events to all registered tracing probes
        cfg.dispatch_events();
        interceptors.read_after_execution(&ctx)?;

        break;
    }

    let (
	    modeled_response,
	    tx_response,
	    property_bag
	) = ctx.into_responses().map_err(SdkError::interceptor_error)?;
    let operation_response = operation::Response::from_parts(
	    tx_response,
	    property_bag
	);

    match modeled_response {
        Ok(output) => Ok(SdkSuccess {
            parsed: output,
            raw: operation_response,
        }),
        Err(err) => Err(SdkError::service_error(
            err,
            operation_response,
        ))
    }
}
```

### The `aws-smithy-interceptors` crate

The `aws-smithy-interceptors` crate is a stub in this RFC that contains only a few types necessary to partially implement the orchestrator. The specific design of interceptors is left to a future RFC.

## F.A.Q.

- The orchestrator is a large and complex feature, with many moving parts. How can we ensure that multiple people can contribute in parallel?
	-
- What is the precedence of interceptors?
	- The precedence of interceptors is as follows:
		- Interceptors registered via Smithy default plugins.
		- *(AWS Services only)* Interceptors registered via AWS default plugins.
		- Interceptors registered via service-customization plugins.
		- Interceptors registered via client-level plugins.
		- Interceptors registered via client-level configuration.
		- Interceptors registered via operation-level plugins.
		- Interceptors registered via operation-level configuration.

## Changes checklist

TODO
