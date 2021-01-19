# HTTP-based Operations
The Smithy code generator for Rust (and by extension), the AWS SDK use an `Operation` abstraction to provide a unified
interface for dispatching requests. `Operation`s contain:
* A base HTTP request (with a potentially streaming body)
* A typed property bag of configuration options
* A fully generic response handler

In the typical case, these configuration options include things like a `CredentialsProvider`, however, they can also be
full middleware layers that will get added by the dispatch stack.

## Operation Phases
This section details the flow of a request through the SDK until a response is returned to the user.

### Input Construction

A customer interacts with the SDK builders to construct an input. The `build()` method on an input returns
an `Operation<Output>`. This codifies the base HTTP request & all the configuration and middleware layers required to modify and dispatch the request.

```rust,ignore
pub struct Operation<H> {
    request: Request,
    response_handler: Box<H>,
}

pub struct Request {
    base: http::Request<SdkBody>,
    configuration: PropertyBag,
}
```

For most requests, `.build()` will NOT consume the input—a user can call `.build()` multiple times to produce multiple operations from the same input.

By using a property bag, we can define the `Operation` in Smithy core–AWS specific configuration can be added later in the stack.

### Operation Construction
In order to construct an operation, the generated code injects appropriate middleware & configuration via the configuration property bag. It does this by reading the configuration properties out of the service
config, copying them as necessary, and loading them into the Request:

```rust,ignore
// This is approximately the generated code—I've cleaned a few things up for readability
pub fn build(self, config: &dynamodb::config::Config) -> Operation<BatchExecuteStatement> {
    let op = BatchExecuteStatement::new(BatchExecuteStatementInput {
        statements: self.statements,
    });
    let mut request = operation::Request::new(
        op.build_http_request()
            .map(|body| operation::SdkBody::from(body)),
    );

    use operation::signing_middleware::SigningConfigExt;
    request
        .config
        .insert_signingconfig(SigningConfig::default_config(
            auth::ServiceConfig {
                service: config.signing_service().into(),
                region: config.region.clone().into(),
            },
            auth::RequestConfig {
                request_ts: || std::time::SystemTime::now(),
            },
        ));
    use operation::signing_middleware::CredentialProviderExt;
    request
        .config
        .insert_credentials_provider(config.credentials_provider.clone());

    use operation::endpoint::EndpointProviderExt;
    request
        .config
        .insert_endpoint_provider(config.endpoint_provider.clone());

    Operation::new(request, op)
}
```
