# Request Lifecycle in aws-smithy-http-server

This document traces the complete lifecycle of an HTTP request through the smithy-rs server framework, from network socket to handler and back.

## Overview

```
┌─────────────────────────────────────────────────────────────────────┐
│                         Request Lifecycle                            │
├─────────────────────────────────────────────────────────────────────┤
│                                                                       │
│  1. Network Layer (Hyper)                                           │
│     ↓ Request<hyper::body::Incoming>                                │
│                                                                       │
│  2. RoutingService<Router, Protocol>                                │
│     ↓ Matches request to operation                                  │
│                                                                       │
│  3. Route<B> (type-erased service)                                  │
│     ↓ BoxCloneService wrapper                                       │
│                                                                       │
│  4. Plugin/Middleware Stack                                          │
│     ↓ HTTP plugins, Model plugins, Custom layers                    │
│                                                                       │
│  5. Upgrade Layer                                                    │
│     ↓ FromRequest deserialization (body → Input type)              │
│     ↓ ⚠️  hyper::Error can occur here!                              │
│                                                                       │
│  6. Handler / IntoService                                            │
│     ↓ User's business logic                                         │
│     ↓ Returns Result<Output, Error>                                 │
│                                                                       │
│  7. Response Path (reversed)                                         │
│     ↓ Serialize Output → Response<BoxBody>                          │
│     ↓ Through plugins/middleware                                     │
│     ↓ Back to Hyper                                                  │
│     ↓ Network transmission                                           │
│                                                                       │
└─────────────────────────────────────────────────────────────────────┘
```

## Detailed Lifecycle

### 1. Network Connection & HTTP Parsing

**Location**: `src/serve/mod.rs:290`

```rust
use hyper::body::Incoming;
```

- TCP connection established via `TcpListener`
- Hyper's HTTP/1 or HTTP/2 parser reads the request
- Request constructed as `http::Request<hyper::body::Incoming>`
- `hyper::body::Incoming` is Hyper's streaming body type
- Error type: `hyper::body::Incoming::Error = hyper::Error`

**Key Point**: This is why we need `convert_to_request_rejection!(hyper::Error, ...)` - Hyper is the source of incoming requests.

### 2. Service Factory (MakeService)

**Location**: `src/routing/into_make_service.rs`

```rust
// User code:
let app = MyService::builder(config)
    .operation_handler(handler)
    .build_unchecked();

let make_service = app.into_make_service();
```

- `IntoMakeService<S>` wraps the service
- For each connection, clones the service
- No per-connection state unless using `into_make_service_with_connect_info()`

### 3. RoutingService Receives Request

**Location**: `src/routing/mod.rs:178-206`

```rust
impl<R, P, B, RespB> Service<http::Request<B>> for RoutingService<R, P>
where
    R: Router<B>,
{
    fn call(&mut self, req: http::Request<B>) -> Self::Future {
        match self.router.match_route(&req) {
            Ok(ok) => RoutingFuture::from_oneshot(ok.oneshot(req)),
            Err(error) => RoutingFuture::from_response(error.into_response())
        }
    }
}
```

**Type**: `RoutingService<RestRouter<Route<hyper::body::Incoming>>, RestJson1>`

The router examines:
- HTTP method
- URI path pattern
- Query parameters (for some protocols)
- Headers (for AWS JSON protocols: `x-amz-target`)

**Outcomes**:
- ✅ Match found → Route to operation's service
- ❌ No match → Return 404 or protocol-specific error

### 4. Router Matching

**Implementations**:
- `src/protocol/rest/router.rs:66-91` - REST protocols (RestJson1, RestXml)
- `src/protocol/aws_json/router.rs:81-103` - AWS JSON 1.0/1.1
- `src/protocol/rpc_v2_cbor/router.rs:203-241` - RPC v2 CBOR

#### REST Router Example:
```rust
impl<B, S> Router<B> for RestRouter<S> {
    fn match_route(&self, request: &http::Request<B>) -> Result<S, Self::Error> {
        for (request_spec, route) in &self.routes {
            match request_spec.matches(request) {
                Match::Yes => return Ok(route.clone()),
                Match::MethodNotAllowed => method_allowed = false,
                Match::No => continue,
            }
        }
        // Return appropriate error
    }
}
```

Routes are sorted by specificity (most specific first).

### 5. Route (Type-Erased Service)

**Location**: `src/routing/route.rs:53-67`

```rust
pub struct Route<B = hyper::body::Incoming> {
    service: BoxCloneService<Request<B>, Response<BoxBody>, Infallible>,
}
```

**Purpose**: Type erasure via `BoxCloneService` (trait object)

Each operation has a complex concrete type like:
```rust
Upgrade<
    RestJson1,
    GetUserInput,
    Layer<Layer<Layer<IntoService<GetUser, ClosureType>>>>
>
```

`Route` erases this to a uniform `BoxCloneService`, enabling:
- Storage in collections: `Vec<(RequestSpec, Route<B>)>`
- Reduced binary size (less monomorphization)
- Simpler type signatures

**Trade-off**: Small dynamic dispatch overhead (negligible vs network I/O)

### 6. Plugin/Middleware Stack

**Location**: Generated in `service.rs`, applied in order:

```rust
// From generated code:
let svc = OperationShape::from_handler(handler);
let svc = self.model_plugin.apply(svc);           // 1. Model plugins
let svc = UpgradePlugin::new().apply(svc);         // 2. Upgrade (FromRequest)
let svc = self.http_plugin.apply(svc);             // 3. HTTP plugins
```

#### Common Middleware:
1. **Model Plugins**: Validation, custom operation logic
2. **HTTP Plugins**: Logging, metrics, authentication
3. **Tower Layers**: Timeouts, rate limiting, tracing

### 7. Upgrade Layer (Request Deserialization)

**Location**: `src/operation/upgrade.rs`

This is the critical layer that performs protocol deserialization:

```rust
impl<Protocol, Op, S, B> Service<http::Request<B>> for Upgrade<Protocol, Op, S>
where
    Op::Input: FromRequest<Protocol, B>,
{
    fn call(&mut self, req: http::Request<B>) -> Self::Future {
        // Deserialize request → Op::Input
        let fut = Op::Input::from_request(req);
        // Then pass to inner service
    }
}
```

#### FromRequest Implementation (Generated)

**Example**: For a REST operation with JSON body:

```rust
impl<B> FromRequest<RestJson1, B> for GetUserInput
where
    B: HttpBody + Send + 'static,
    B::Data: Send,
    B::Error: Into<BoxError>,
{
    type Rejection = RequestRejection;

    async fn from_request(req: Request<B>) -> Result<Self, Self::Rejection> {
        // 1. Extract path parameters
        let path_params = extract_path(&req)?;

        // 2. Extract query parameters
        let query_params = extract_query(&req)?;

        // 3. Extract headers
        let headers = extract_headers(&req)?;

        // 4. Read and deserialize body
        let body_bytes = http_body_util::BodyExt::collect(req.into_body())
            .await
            .map_err(Error::new)?  // ⚠️ hyper::Error → crate::Error
            .to_bytes();

        let input: InputShape = serde_json::from_slice(&body_bytes)
            .map_err(|e| RequestRejection::JsonDeserialize(e))?;

        // 5. Apply constraint validation
        input.try_into()  // Converts to constrained type
    }
}
```

**❗ Key Point**: This is where `hyper::Error` occurs!

When reading the body:
```rust
http_body_util::BodyExt::collect(req.into_body()).await
```

This can fail with:
- `hyper::Error` if using `Request<hyper::body::Incoming>`
- Custom error types if using custom body types

The error flows:
```
hyper::Error
  ↓ (via .map_err(Error::new))
crate::Error
  ↓ (via From<crate::Error> for RequestRejection)
RequestRejection::BufferHttpBodyBytes(crate::Error)
```

This is why we need:
```rust
convert_to_request_rejection!(hyper::Error, BufferHttpBodyBytes);
```

### 8. Handler Execution & Extractors

**Location**: `src/operation/handler.rs`, `src/request/mod.rs`

#### Handler Trait

```rust
pub trait Handler<Op, Exts> {
    type Future: Future<Output = Result<Op::Output, Op::Error>>;
    fn call(&mut self, input: Op::Input, exts: Exts) -> Self::Future;
}
```

The `Exts` type parameter allows handlers to receive **additional parameters** beyond just the operation input.

#### The `impl_handler!` Macro

**Location**: `src/operation/handler.rs:65-93`

This macro generates `Handler` implementations for functions with 0 to 8 additional parameters:

```rust
macro_rules! impl_handler {
    ($($var:ident),+) => (
        impl<Op, F, Fut, $($var,)*> Handler<Op, ($($var,)*)> for F
        where
            Op: OperationShape,
            F: Fn(Op::Input, $($var,)*) -> Fut,
            Fut: Future,
            Fut::Output: IntoResult<Op::Output, Op::Error>,
        {
            fn call(&mut self, input: Op::Input, exts: ($($var,)*)) -> Self::Future {
                let ($($var,)*) = exts;  // Unpack the tuple
                (self)(input, $($var,)*).map(IntoResult::into_result)
            }
        }
    )
}

impl_handler!(Exts0);
impl_handler!(Exts0, Exts1);
// ... up to 8 parameters
impl_handler!(Exts0, Exts1, Exts2, Exts3, Exts4, Exts5, Exts6, Exts7, Exts8);
```

#### Handler Examples

```rust
// No extractors - just operation input
async fn handler1(input: GetUserInput) -> Result<GetUserOutput, GetUserError> {
    // Business logic
    Ok(GetUserOutput { /* ... */ })
}

// One extractor - connection info
async fn handler2(
    input: GetUserInput,
    conn_info: ConnectInfo<SocketAddr>,
) -> Result<GetUserOutput, GetUserError> {
    tracing::info!("Request from: {}", conn_info.0);
    Ok(GetUserOutput { /* ... */ })
}

// Multiple extractors
async fn handler3(
    input: GetUserInput,
    conn_info: ConnectInfo<SocketAddr>,
    db: Extension<Database>,
    request_id: ServerRequestId,
    ctx: Option<Extension<CustomContext>>, // Optional extractor
) -> Result<GetUserOutput, GetUserError> {
    tracing::info!(request_id = %request_id, "Processing request");
    let user = db.find_user(input.user_id).await?;
    Ok(GetUserOutput { user })
}
```

#### The `FromParts` Trait

**Location**: `src/request/mod.rs:82-88`

Extractors must implement `FromParts` to be extracted from request parts (not body):

```rust
pub trait FromParts<Protocol>: Sized {
    type Rejection: IntoResponse<Protocol>;

    /// Extracts `self` from request parts synchronously
    fn from_parts(parts: &mut Parts) -> Result<Self, Self::Rejection>;
}
```

**Key Point**: `FromParts` is **synchronous** and extracts from `http::request::Parts` (headers, extensions, URI, etc.), NOT from the body.

#### Built-in Extractors

**Location**: `src/request/`

| Extractor | Module | Description |
|-----------|--------|-------------|
| `ConnectInfo<T>` | `connect_info` | Connection metadata (socket address, etc.) |
| `Extension<T>` | `extension` | Custom data from request extensions |
| `ServerRequestId` | `request_id` | Server-generated request ID |
| `Context` | `lambda` | AWS Lambda context (with `aws-lambda` feature) |
| `ApiGatewayProxyRequestContext` | `lambda` | API Gateway v1 context |
| `ApiGatewayV2httpRequestContext` | `lambda` | API Gateway v2 context |
| `Option<T>` | `mod` | Optional extraction (returns `None` if fails) |
| `Result<T, T::Rejection>` | `mod` | Captures extraction result |

#### Example: `ConnectInfo` Implementation

**Location**: `src/request/connect_info.rs:48-56`

```rust
impl<P, T> FromParts<P> for ConnectInfo<T>
where
    T: Send + Sync + 'static,
{
    type Rejection = MissingConnectInfo;

    fn from_parts(parts: &mut Parts) -> Result<Self, Self::Rejection> {
        parts.extensions.remove().ok_or(MissingConnectInfo)
        //    ^^^^^^^^^^^ Extracts from request extensions
    }
}
```

#### Extractor Extraction Order

**Location**: `src/request/mod.rs:162-197`

When a handler has multiple extractors, they're extracted in a specific order:

```rust
impl<P, B, T1, T2> FromRequest<P, B> for (T1, T2)
where
    T1: FromRequest<P, B>,  // First param: consumes body
    T2: FromParts<P>,       // Second param: from parts
{
    fn from_request(request: Request<B>) -> Self::Future {
        let (mut parts, body) = request.into_parts();

        // 1. Extract T2 from parts (synchronous)
        let t2_result = T2::from_parts(&mut parts);

        // 2. Extract T1 from full request (asynchronous, may read body)
        let t1_future = T1::from_request(Request::from_parts(parts, body));

        // 3. Combine both results
        try_join(t1_future, ready(t2_result))
    }
}
```

**Order**:
1. **Parts extractors** extracted first (from headers, extensions, etc.)
2. **Input extraction** happens last (may consume body)
3. If any extraction fails, handler is never called

#### Tuple Implementations

The `FromParts` trait also has tuple implementations (up to 8 elements):

**Location**: `src/request/mod.rs:109-135`

```rust
impl<P, T1, T2, T3> FromParts<P> for (T1, T2, T3)
where
    T1: FromParts<P>,
    T2: FromParts<P>,
    T3: FromParts<P>,
{
    fn from_parts(parts: &mut Parts) -> Result<Self, Self::Rejection> {
        Ok((
            T1::from_parts(parts)?,
            T2::from_parts(parts)?,
            T3::from_parts(parts)?,
        ))
    }
}
```

This allows the handler's extractor tuple to be decomposed and each extracted individually.

#### Handler Storage

The handler is not stored as `Fn` directly, but wrapped in `IntoService`:

```rust
pub struct IntoService<Op, H> {
    pub(crate) handler: H,  // Original closure stored here
    pub(crate) _operation: PhantomData<Op>,
}

impl<Op, Exts, H> Service<(Op::Input, Exts)> for IntoService<Op, H>
where
    H: Handler<Op, Exts>,
{
    fn call(&mut self, (input, exts): (Op::Input, Exts)) -> Self::Future {
        self.handler.call(input, exts)  // Unpacks tuple and calls handler
    }
}
```

#### Complete Extraction Flow

For a handler like:
```rust
async fn handler(
    input: GetUserInput,
    conn: ConnectInfo<SocketAddr>,
    db: Extension<Database>,
) -> Result<GetUserOutput, GetUserError>
```

The extraction flow is:

```
Request<hyper::body::Incoming>
  ↓
Split into Parts + Body
  ↓
FromParts<P> for (ConnectInfo<SocketAddr>, Extension<Database>)
  ├─ ConnectInfo::from_parts(&mut parts)  ✅ Extracted from extensions
  └─ Extension::from_parts(&mut parts)     ✅ Extracted from extensions
  ↓
FromRequest<P, B> for GetUserInput
  └─ Consumes body, deserializes JSON/CBOR → GetUserInput  ✅
  ↓
Combine into tuple: (GetUserInput, (ConnectInfo, Extension))
  ↓
IntoService::call((input, exts))
  ├─ Unpacks: input, conn, db
  └─ Calls: handler(input, conn, db)
  ↓
Handler executes with all parameters
```

#### Error Handling in Extractors

If any extractor fails, the handler is **never called**:

```rust
// In UpgradeFuture (src/operation/upgrade.rs:142-147)
match result {
    Ok(ok) => {
        // All extractors succeeded, call handler
        this.service.take().unwrap().oneshot(ok)
    }
    Err(err) => {
        // Extractor failed, return error response immediately
        tracing::trace!(error = %err, "parameter for the handler cannot be constructed");
        return Poll::Ready(Ok(err.into_response()));
    }
}
```

Common extraction failures:
- `MissingConnectInfo` - Used `ConnectInfo` but didn't call `.into_make_service_with_connect_info()`
- `MissingExtension` - Used `Extension<T>` but didn't add via middleware
- `MissingServerRequestId` - Used `ServerRequestId` but didn't enable `request-id` feature

#### Optional and Result Extractors

**Location**: `src/request/mod.rs:199-218`

Special wrapper types for graceful failure handling:

```rust
// Option<T> - Returns None if extraction fails
async fn handler(
    input: GetUserInput,
    maybe_ctx: Option<Extension<Context>>,  // Won't fail if missing
) -> Result<GetUserOutput, GetUserError> {
    if let Some(ctx) = maybe_ctx {
        // Use context
    }
    // Continue without context
}

// Result<T, T::Rejection> - Captures the error
async fn handler(
    input: GetUserInput,
    conn_result: Result<ConnectInfo<SocketAddr>, MissingConnectInfo>,
) -> Result<GetUserOutput, GetUserError> {
    match conn_result {
        Ok(conn) => { /* use conn info */ }
        Err(e) => { /* handle missing connection info */ }
    }
}
```

### 9. Response Serialization

After the handler returns `Result<Output, Error>`, the response is serialized:

#### Success Path

```rust
impl IntoResponse<RestJson1> for GetUserOutput {
    fn into_response(self) -> http::Response<BoxBody> {
        let body = serde_json::to_vec(&self).unwrap();
        http::Response::builder()
            .status(200)
            .header("content-type", "application/json")
            .body(body::to_boxed(body))
            .unwrap()
    }
}
```

#### Error Path

```rust
impl IntoResponse<RestJson1> for GetUserError {
    fn into_response(self) -> http::Response<BoxBody> {
        match self {
            GetUserError::NotFound(err) => {
                let body = serde_json::to_vec(&err).unwrap();
                http::Response::builder()
                    .status(404)
                    .body(body::to_boxed(body))
                    .unwrap()
            }
        }
    }
}
```

### 10. Response Body Wrapping

**Location**: `src/body.rs:40-48`

All response bodies are converted to `BoxBody`:

```rust
pub type BoxBody = http_body_util::combinators::UnsyncBoxBody<Bytes, Error>;

pub fn boxed<B>(body: B) -> BoxBody
where
    B: http_body::Body<Data = Bytes> + Send + 'static,
    B::Error: Into<BoxError>,
{
    body.map_err(Error::new).boxed_unsync()
}
```

**Key Point**: Response bodies use `crate::Error`, NOT `hyper::Error`.

The error transformation:
```
BodyImpl::Error (e.g., std::io::Error)
  ↓ (via .map_err(Error::new))
crate::Error
  ↓ (wrapped in BoxBody)
BoxBody = UnsyncBoxBody<Bytes, Error>
```

### 11. Through Middleware (Return Path)

Response travels back through the plugin stack in reverse order:
1. HTTP plugins can modify response
2. Upgrade layer doesn't touch response
3. Model plugins can add headers, log, etc.

### 12. Back to Hyper

**Location**: `src/serve/mod.rs`

```rust
// TowerToHyperService converts Tower Service → Hyper Service
let hyper_service = TowerToHyperService::new(svc);

// Hyper sends response over the wire
```

Hyper:
1. Serializes HTTP response headers
2. Streams the body chunks
3. Handles connection management (keep-alive, etc.)

## Body Type Flow

### Request Bodies

```
┌─────────────────────────────────────────────────────────────────┐
│                         Request Body Flow                        │
├─────────────────────────────────────────────────────────────────┤
│                                                                   │
│  hyper::body::Incoming                                           │
│     Error: hyper::Error                                          │
│     ↓                                                             │
│  FromRequest reads body                                          │
│     .collect().await → Result<Collected, hyper::Error>          │
│     ↓                                                             │
│  Error conversion (if needed)                                    │
│     hyper::Error → crate::Error → RequestRejection              │
│     ↓                                                             │
│  Deserialized to Input type                                      │
│     Bytes → serde_json::from_slice → InputStruct                │
│                                                                   │
└─────────────────────────────────────────────────────────────────┘
```

### Response Bodies

```
┌─────────────────────────────────────────────────────────────────┐
│                        Response Body Flow                        │
├─────────────────────────────────────────────────────────────────┤
│                                                                   │
│  Handler returns Output                                          │
│     ↓                                                             │
│  Serialize to bytes                                              │
│     serde_json::to_vec(&output) → Vec<u8>                       │
│     ↓                                                             │
│  Wrap in Full body                                               │
│     http_body_util::Full<Bytes>                                 │
│     Error: Infallible                                            │
│     ↓                                                             │
│  Convert to BoxBody                                              │
│     .map_err(Error::new).boxed_unsync()                         │
│     Error: crate::Error                                          │
│     ↓                                                             │
│  BoxBody type                                                    │
│     UnsyncBoxBody<Bytes, Error>                                 │
│                                                                   │
└─────────────────────────────────────────────────────────────────┘
```

## Event Streaming

For operations with event streams, the flow is modified:

### Request with Event Stream

```rust
pub struct StreamingInput {
    pub events: Receiver<EventType>,
}

impl<B> FromRequest<Protocol, B> for StreamingInput {
    async fn from_request(req: Request<B>) -> Result<Self, Self::Rejection> {
        let (sender, receiver) = channel();

        // Body is NOT consumed here - passed as receiver
        let events = EventStreamReceiver::new(req.into_body());

        Ok(StreamingInput { events: receiver })
    }
}
```

### Response with Event Stream

```rust
pub struct StreamingOutput {
    pub events: EventStreamSender<EventType>,
}

impl IntoResponse<Protocol> for StreamingOutput {
    fn into_response(self) -> http::Response<BoxBody> {
        let stream = self.events.into_stream();
        let body = body::wrap_stream(stream);  // Stream → Body

        http::Response::builder()
            .status(200)
            .body(body)
            .unwrap()
    }
}
```

**Location**: `src/body.rs:126-141`

```rust
pub fn wrap_stream<S, O, E>(stream: S) -> BoxBody
where
    S: futures_util::Stream<Item = Result<O, E>> + Send + 'static,
    O: Into<Bytes> + 'static,
    E: Into<BoxError> + 'static,
{
    let frame_stream = stream
        .map_ok(|chunk| http_body::Frame::data(chunk.into()))
        .map_err(|e| Error::new(e.into()));

    boxed(StreamBody::new(frame_stream))
}
```

## Error Handling Throughout

### Request Path Errors

| Layer | Error Type | Conversion |
|-------|------------|------------|
| Hyper body reading | `hyper::Error` | → `RequestRejection::BufferHttpBodyBytes` |
| JSON deserialization | `serde_json::Error` | → `RequestRejection::JsonDeserialize` |
| Path parsing | `nom::Err` | → `RequestRejection::UriPatternMismatch` |
| Constraint validation | `ConstraintViolation` | → `RequestRejection::ConstraintViolation` |
| Router matching | `router::Error` | → Protocol-specific error response |

### Response Path Errors

Response bodies use `crate::Error`, but errors are rare because:
1. Serialization to `Vec<u8>` happens in memory (infallible for valid types)
2. Only stream bodies can error during transmission
3. Errors after response headers sent → connection dropped

## Why `hyper::Error` Conversion Is Needed

**The Question**: Since we have `From<crate::Error> for RequestRejection`, why do we need `convert_to_request_rejection!(hyper::Error, ...)`?

**The Answer**: Rust's type inference and error propagation.

### Scenario 1: With explicit conversion

```rust
async fn from_request<B>(req: Request<B>) -> Result<Input, RequestRejection>
where
    B::Error = hyper::Error,  // Hyper's body
{
    let body_bytes = req.into_body()
        .collect().await  // Returns Result<Collected, hyper::Error>
        .map_err(Error::new)?;  // hyper::Error → crate::Error
        // ↑ This works: From<crate::Error> for RequestRejection

    Ok(deserialize(body_bytes)?)
}
```

### Scenario 2: Using `?` operator directly

```rust
async fn from_request<B>(req: Request<B>) -> Result<Input, RequestRejection>
where
    B::Error = hyper::Error,
{
    let body_bytes = req.into_body()
        .collect().await?;  // ❌ Error: hyper::Error doesn't impl Into<RequestRejection>

    Ok(deserialize(body_bytes)?)
}
```

Without `From<hyper::Error> for RequestRejection`, the `?` operator fails.

**But wait**: We have `From<crate::Error>`, can't `hyper::Error` → `crate::Error` → `RequestRejection`?

**No**: Rust doesn't chain multiple `From` impls automatically. The `?` operator only looks for a single `From` impl.

### Solution

```rust
convert_to_request_rejection!(hyper::Error, BufferHttpBodyBytes);

// Expands to:
impl From<hyper::Error> for RequestRejection {
    fn from(err: hyper::Error) -> Self {
        Self::BufferHttpBodyBytes(crate::Error::new(err))
    }
}
```

Now the `?` operator can directly convert `hyper::Error` → `RequestRejection`.

## Type Parameter: Why `Route<B = hyper::body::Incoming>`?

### The Default

```rust
pub struct Route<B = hyper::body::Incoming> {
    service: BoxCloneService<Request<B>, Response<BoxBody>, Infallible>,
}
```

### Why Default to `hyper::body::Incoming`?

**It's pure ergonomics**:

```rust
// Without default:
let router: RestRouter<Route<hyper::body::Incoming>> = ...;
let app: MyService<RoutingService<RestRouter<Route<hyper::body::Incoming>>, Protocol>> = ...;

// With default:
let router: RestRouter<Route> = ...;
let app: MyService<RoutingService<RestRouter<Route>, Protocol>> = ...;
```

### Why Not Leave It Fully Generic?

The framework IS generic - `Route<B>` can use any body type:

```rust
// Custom body type:
struct MyCustomBody { /* ... */ }

impl HttpBody for MyCustomBody {
    type Data = Bytes;
    type Error = MyError;
    // ...
}

// Use it:
let route: Route<MyCustomBody> = Route::new(my_service);
```

The default just makes the common case (using Hyper) more ergonomic.

### Relationship to `hyper::Error` Conversion

The default is **independent** from the error conversion need:

- **Default exists**: For ergonomics when writing type signatures
- **Conversion needed**: Because Hyper's HTTP server uses `Incoming` body

Even without the default, we'd need the conversion because:
1. Hyper provides `Request<hyper::body::Incoming>`
2. `Incoming::Error = hyper::Error`
3. Users (and generated code) use Hyper servers

## Performance Characteristics

### Zero-Cost Abstractions

✅ **Truly zero-cost**:
- Handler trait via blanket impl (monomorphized)
- Plugin composition (compile-time)
- Default type parameters (compile-time)

⚠️ **Small overhead**:
- `BoxCloneService` in `Route` (dynamic dispatch)
- Tokio task spawning per connection

❌ **Not zero-cost** (but unavoidable):
- HTTP parsing (Hyper)
- JSON serialization/deserialization (serde)
- Network I/O

### Compilation Impacts

**Without type erasure** (`Route`):
```
Router<(Operation1Type, Operation2Type, ..., Operation100Type)>
```
- 100 different concrete types
- Monomorphization explosion
- Long compile times
- Large binary size

**With type erasure** (`Route`):
```
Router<Route<B>>
```
- One uniform type
- Minimal monomorphization
- Fast compile times
- Small binary size

**Trade-off**: Virtual dispatch overhead (~1-2 nanoseconds per call, negligible vs network latency)

## Summary

A request flows through approximately 10 distinct layers:

1. **Network** → TCP socket
2. **Hyper** → HTTP parsing → `Request<Incoming>`
3. **MakeService** → Service factory per connection
4. **RoutingService** → Protocol-specific routing
5. **Router** → Pattern matching to operation
6. **Route** → Type-erased service wrapper
7. **Middleware** → User-defined plugins/layers
8. **Upgrade** → Request deserialization (⚠️ `hyper::Error` here!)
9. **Handler** → User business logic
10. **Response** → Serialization → `Response<BoxBody>`

Then back through layers 7-1 to the client.

**Critical Error Points**:
- **Layer 8 (Upgrade)**: Body reading can fail with `hyper::Error`
- **Layer 9 (Handler)**: Business logic can fail with operation errors
- **Layer 5 (Router)**: Routing can fail with 404/405 errors

This architecture provides:
- ✅ Type safety (compile-time protocol checks)
- ✅ Performance (zero-cost abstractions where possible)
- ✅ Ergonomics (good error messages, clear APIs)
- ✅ Extensibility (Tower middleware ecosystem)
- ✅ Correctness (Smithy protocol compliance)
