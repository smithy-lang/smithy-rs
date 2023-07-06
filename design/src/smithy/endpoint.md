# Endpoint Resolution

## Requirements
The core codegen generates HTTP requests that do not contain an authority, scheme or post. These properties must be set later based on configuration. Existing AWS services have a number of requirements that increase the complexity:

1. Endpoints must support manual configuration by end users:
```rust,ignore
let config = dynamodb::Config::builder()
    .endpoint(StaticEndpoint::for_uri("http://localhost:8000"))
```

When a user specifies a custom endpoint URI, _typically_ they will want to avoid having this URI mutated by other endpoint discovery machinery.

2. Endpoints must support being customized on a per-operation basis by the endpoint trait. This will prefix the base endpoint, potentially driven by fields of the operation. [Docs](https://awslabs.github.io/smithy/1.0/spec/core/endpoint-traits.html#endpoint-trait)

3. Endpoints must support being customized by [endpoint discovery](https://awslabs.github.io/smithy/1.0/spec/aws/aws-core.html#client-endpoint-discovery). A request, customized by a predefined set of fields from the input operation is dispatched to a specific URI. That operation returns the endpoint that should be used. Endpoints must be cached by a cache key containing:
```markdown
(access_key_id, [all input fields], operation)
```
Endpoints retrieved in this way specify a TTL.

4. Endpoints must be able to customize the signing (and other phases of the operation). For example, requests sent to a global region will have a region set by the endpoint provider.


## Design

Configuration objects for services _must_ contain an `Endpoint`. This endpoint may be set by a user or it will default to the `endpointPrefix` from the service definition. In the case of endpoint discovery, _this_ is the endpoint that we will start with.

During operation construction (see [Operation Construction](../transport/operation.md#operation-construction)) an `EndpointPrefix` may be set on the property bag. The eventual endpoint middleware will search for this in the property bag and (depending on the URI mutability) utilize this prefix when setting the endpoint.

In the case of endpoint discovery, we envision a different pattern:
```rust,ignore
// EndpointClient manages the endpoint cache
let (tx, rx) = dynamodb::EndpointClient::new();
let client = aws_hyper::Client::new();
// `endpoint_req` is an operation that can be dispatched to retrieve endpoints
// During operation construction, the endpoint resolver is configured to be `rx` instead static endpoint
// resolver provided by the service.
let (endpoint_req, req) = GetRecord::builder().endpoint_disco(rx).build_with_endpoint();
// depending on the duration of endpoint expiration, this may be spawned into a separate task to continuously
// refresh endpoints.
if tx.needs(endpoint_req) {
    let new_endpoint = client.
        call(endpoint_req)
        .await;
    tx.send(new_endpoint)
}
let rsp = client.call(req).await?;
```

We believe that this design results in an SDK that both offers customers more control & reduces the likelihood of bugs from nested operation dispatch. Endpoint resolution is currently extremely rare in AWS services so this design may remain a prototype while we solidify other behaviors.
