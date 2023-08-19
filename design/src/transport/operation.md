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
pub struct Operation<H, R> {
    request: Request,
    response_handler: H,
    _retry_policy: R,
}

pub struct Request {
    inner: http::Request<SdkBody>,
    properties: PropertyBag,
}
```

For most requests, `.build()` will NOT consume the input. A user can call `.build()` multiple times to produce multiple operations from the same input.

By using a property bag, we can define the `Operation` in Smithy core. AWS specific configuration can be added later in the stack.

### Operation Construction
In order to construct an operation, the generated code injects appropriate middleware & configuration via the configuration property bag. It does this by reading the configuration properties out of the service
config, copying them as necessary, and loading them into the `Request`:

```rust,ignore
// This is approximately the generated code, I've cleaned a few things up for readability.
pub fn build(self, config: &dynamodb::config::Config) -> Operation<BatchExecuteStatement> {
    let op = BatchExecuteStatement::new(BatchExecuteStatementInput {
        statements: self.statements,
    });
    let req = op.build_http_request().map(SdkBody::from);

    let mut req = operation::Request::new(req);
    let mut props = req.properties_mut();
    props.insert_signing_config(config.signing_service());
    props.insert_endpoint_resolver(config.endpoint_resolver.clone());
    Operation::new(req)
}
```

### Operation Dispatch and Middleware

The Rust SDK endeavors to behave as predictably as possible. This means that if at all possible we will not dispatch extra HTTP requests during the dispatch of normal operation. Making this work is covered in more detail in the design of credentials providers & endpoint resolution.

The upshot is that we will always prefer a design where the user has explicit control of when credentials are loaded and endpoints are resolved. This doesn't mean that users can't use easy-to-use options (We will provide an automatically refreshing credentials provider), however, the credential provider won't load requests during the dispatch of an individual request.

## Operation Parsing and Response Loading

The fundamental trait for HTTP-based protocols is `ParseHttpResponse`
