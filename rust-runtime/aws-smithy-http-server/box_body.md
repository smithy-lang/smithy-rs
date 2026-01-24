# Body Type Abstraction Plan

## Executive Summary

This document analyzes the body type handling in `aws-smithy-http-server` and outlines changes to decouple the public API from hyper's `Incoming` type.

### Current State

| Component | Body Type | Status |
|-----------|-----------|--------|
| Generated SDKs | Generic `Body` parameter | Already flexible |
| `Router<B>` trait | Generic over `B` | Already flexible |
| `RoutingService` | Generic over `B` | Already flexible |
| `Route<B>` struct | Generic over `B` | Already flexible |
| Built-in middleware | Generic (`<B>`, `<Body>`) | Already flexible |
| `Route` default type | `= hyper::body::Incoming` | **Needs change** |
| `serve()` function | Bound to `Incoming` | **Needs change** (later) |

### Key Finding

The routing infrastructure is already well-designed and generic. The `Incoming` coupling exists only at two edges:
1. `Route`'s default type parameter
2. `serve()` function's type bounds

---

## 1. Why Router Infrastructure Is Already Correct

### 1.1 Generic Router Trait

```rust
// src/routing/mod.rs:68-74
pub trait Router<B> {
    type Service;
    type Error;
    fn match_route(&self, request: &http::Request<B>) -> Result<Self::Service, Self::Error>;
}
```

### 1.2 Generic RoutingService

```rust
// src/routing/mod.rs:176-186
impl<R, P, B, RespB> Service<http::Request<B>> for RoutingService<R, P>
where
    R: Router<B>,
    R::Service: Service<http::Request<B>, Response = http::Response<RespB>> + Clone,
    R::Error: IntoResponse<P> + Error,
    RespB: HttpBody<Data = Bytes> + Send + 'static,
    RespB::Error: Into<BoxError>,
```

### 1.3 Generic Route

```rust
// src/routing/route.rs:83-97
impl<B> Service<Request<B>> for Route<B> {
    type Response = Response<BoxBody>;
    type Error = Infallible;
    type Future = RouteFuture<B>;
    // ...
}
```

This design matches Axum's approach where `Router` implements `Service<Request<B>>` for any `B: HttpBody`.

---

## 2. Generated SDKs Are Body-Agnostic

Generated Smithy SDKs already use generic type parameters:

```rust
// From examples/pokemon-service-server-sdk/src/service.rs
pub struct PokemonServiceBuilder<Body, L, HttpPl, ModelPl> {
    capture_pokemon: Option<::aws_smithy_http_server::routing::Route<Body>>,
    check_health: Option<::aws_smithy_http_server::routing::Route<Body>>,
    // ... all routes use generic Body
}

// Service bounds use generic Body
HttpPl::Output: ::tower::Service<
    ::http_1x::Request<Body>,  // Generic Body, not Incoming
    Response = ::http_1x::Response<::aws_smithy_http_server::body::BoxBody>,
    Error = ::std::convert::Infallible
>

// FromRequest implementations are generic
impl<B> ::aws_smithy_http_server::request::FromRequest<RestJson1, B>
    for crate::input::GetStorageInput
where
    B: ::aws_smithy_http_server::body::HttpBody + Send + 'static,
    B::Data: Send,
```

**No codegen changes are required.**

---

## 3. Built-in Middleware Is Already Generic

| Middleware | Body Type Parameter | File |
|------------|---------------------|------|
| `AlbHealthCheckService` | `<B>` | `src/layer/alb_health_check.rs` |
| `ServerRequestIdProvider` | `<Body>` | `src/request/request_id.rs` |
| `InstrumentOperation` | `<U>` | `src/instrumentation/service.rs` |
| `OperationExtensionService` | `<B>` | `src/extension.rs` |
| Plugin system | Body-agnostic | `src/plugin/` |

All middleware accepts any body type implementing `HttpBody`.

---

## 4. The Route Default Type Parameter

### Current Definition

```rust
// src/routing/route.rs:52
pub struct Route<B = hyper::body::Incoming> {
    service: BoxCloneService<Request<B>, Response<BoxBody>, Infallible>,
}
```

### Why This Default Exists

The default `= hyper::body::Incoming` was set because:
1. When using `serve()`, hyper provides `Request<Incoming>` from the network
2. It was a convenience so users could write `Route` instead of `Route<Incoming>`
3. Documentation examples could omit the type parameter

### Why It Should Change

1. **Couples public API to hyper** - `hyper::body::Incoming` appears in public type signatures
2. **Sends wrong signal** - Suggests `Incoming` is the "expected" body type
3. **Generated code doesn't use it** - SDKs always specify the type explicitly
4. **Custom loops may use different types** - Users bypassing `serve()` might use other body types

### Axum's Approach

Axum's `Router` has no default body type - it's purely generic:

```rust
// Axum: No default, fully generic
impl<B> Service<Request<B>> for Router<()>
where
    B: HttpBody<Data = bytes::Bytes> + Send + 'static,
    B::Error: Into<axum_core::BoxError>,
```

Axum's low-level examples (TLS, custom loops) pass `Request<Incoming>` directly to the Router, and it works because the Router is generic.

### Recommended Change

**Option A: Remove the default entirely**

```rust
pub struct Route<B> {
    service: BoxCloneService<Request<B>, Response<BoxBody>, Infallible>,
}
```

This is a minor breaking change but aligns with:
- Generated SDK patterns (always specify type)
- Axum's design (no default)
- Decoupling from hyper

**Option B: Change default to `BoxBody`**

```rust
pub struct Route<B = BoxBody> {
    service: BoxCloneService<Request<B>, Response<BoxBody>, Infallible>,
}
```

Uses an existing smithy type instead of hyper's type.

---

## 5. Body Wrapper Implementation (For Future Use)

If we decide to transform `Incoming` → `Body` in `serve()`, here's the implementation:

### 5.1 The `try_downcast` Function (Already Exists)

Already implemented in `src/body.rs:61`. Used by `boxed()` to avoid double-boxing:

```rust
// src/body.rs:61-72
pub(crate) fn try_downcast<T, K>(k: K) -> Result<T, K>
where
    T: 'static,
    K: Send + 'static,
{
    let mut k = Some(k);
    if let Some(k) = <dyn std::any::Any>::downcast_mut::<Option<T>>(&mut k) {
        Ok(k.take().unwrap())
    } else {
        Err(k.unwrap())
    }
}
```

### 5.2 The `Body` Struct

```rust
use bytes::Bytes;
use http_body::Body as HttpBody;
use http_body::Frame;
use std::pin::Pin;
use std::task::{Context, Poll};

/// The body type used in aws-smithy-http-server requests and responses.
///
/// This is a type-erased wrapper around any `HttpBody` implementation.
/// Uses dynamic dispatch internally but provides a unified type for
/// the entire request/response pipeline.
#[derive(Debug)]
pub struct Body(BoxBody);

impl Body {
    /// Create a new `Body` that wraps another [`http_body::Body`].
    ///
    /// If the input is already a `Body`, it is returned directly without
    /// additional boxing (zero-cost via `try_downcast`).
    pub fn new<B>(body: B) -> Self
    where
        B: HttpBody<Data = Bytes> + Send + 'static,
        B::Error: Into<crate::BoxError>,
    {
        try_downcast(body).unwrap_or_else(|body| Self(boxed(body)))
    }

    /// Create an empty body.
    pub fn empty() -> Self {
        Self::new(http_body_util::Empty::new().map_err(|never| match never {}))
    }

    /// Create a new `Body` from a [`Stream`] of bytes.
    pub fn from_stream<S>(stream: S) -> Self
    where
        S: futures_core::Stream<Item = Result<Bytes, crate::BoxError>> + Send + 'static,
    {
        Self::new(http_body_util::StreamBody::new(
            stream.map(|result| result.map(Frame::data))
        ))
    }
}

impl Default for Body {
    fn default() -> Self {
        Self::empty()
    }
}

impl HttpBody for Body {
    type Data = Bytes;
    type Error = crate::Error;

    #[inline]
    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        Pin::new(&mut self.0).poll_frame(cx)
    }

    #[inline]
    fn size_hint(&self) -> http_body::SizeHint {
        self.0.size_hint()
    }

    #[inline]
    fn is_end_stream(&self) -> bool {
        self.0.is_end_stream()
    }
}
```

### 5.3 Convenient `From` Implementations

```rust
impl From<()> for Body {
    fn from(_: ()) -> Self {
        Self::empty()
    }
}

impl From<&'static str> for Body {
    fn from(s: &'static str) -> Self {
        Self::new(http_body_util::Full::from(s))
    }
}

impl From<String> for Body {
    fn from(s: String) -> Self {
        Self::new(http_body_util::Full::from(s))
    }
}

impl From<Vec<u8>> for Body {
    fn from(v: Vec<u8>) -> Self {
        Self::new(http_body_util::Full::from(v))
    }
}

impl From<Bytes> for Body {
    fn from(b: Bytes) -> Self {
        Self::new(http_body_util::Full::from(b))
    }
}

impl From<BoxBody> for Body {
    fn from(b: BoxBody) -> Self {
        Self(b)
    }
}

// Critical: allows Body::new(incoming) to work
impl From<hyper::body::Incoming> for Body {
    fn from(incoming: hyper::body::Incoming) -> Self {
        Self::new(incoming)
    }
}
```

---

## 6. The `serve()` Function (Deferred)

The `serve()` function currently requires `Service<Request<Incoming>>`:

```rust
// src/serve/mod.rs:539
S: Service<http::Request<Incoming>, Response = http::Response<B>, Error = Infallible>
```

### Options

#### Option A: Transform to `Body` wrapper (like Axum)

Transform `Incoming` → `Body` in `handle_connection` before calling user service:

```rust
// In handle_connection, after getting tower_service:
let tower_service = tower_service.map_request(|req: http::Request<Incoming>| {
    req.map(Body::new)
});
```

Update bounds:
```rust
S: Service<http::Request<Body>, Response = http::Response<B>, Error = Infallible>
```

**Pros:** Users never see `Incoming`, consistent with Axum
**Cons:** Always pays boxing cost, even if user would accept `Incoming`

#### Option B: Generic over request body with conversion trait

Make `serve()` generic over the request body type:

```rust
pub fn serve<L, M, S, ReqBody, ResBody>(listener: L, make_service: M) -> Serve<L, M, S, ReqBody, ResBody>
where
    L: Listener,
    // Request body: any type that can be created from Incoming
    ReqBody: From<Incoming> + HttpBody + Send + 'static,
    ReqBody::Data: Send,
    ReqBody::Error: Into<Box<dyn StdError + Send + Sync>>,
    // Response body
    ResBody: HttpBody + Send + 'static,
    ResBody::Data: Send,
    ResBody::Error: Into<Box<dyn StdError + Send + Sync>>,
    // Service accepts the generic request body
    S: Service<http::Request<ReqBody>, Response = http::Response<ResBody>, Error = Infallible> + Clone + Send + 'static,
    S::Future: Send,
    // MakeService
    M: for<'a> Service<IncomingStream<'a, L>, Error = Infallible, Response = S>,
```

Then in `handle_connection`:
```rust
let tower_service = tower_service.map_request(|req: http::Request<Incoming>| {
    req.map(ReqBody::from)
});
```

**Pros:**
- User chooses: `Body` (wrapped) or `Incoming` (zero-cost via identity `From` impl)
- Maximum flexibility

**Cons:**
- More complex signature
- Needs `impl From<Incoming> for Incoming` (identity impl, or use a different trait)

#### Option C: Keep as-is

`serve()` is optional. Users who want custom body handling can write their own accept loop:

```rust
// Custom accept loop (like Axum's low-level examples)
loop {
    let (stream, addr) = listener.accept().await?;
    let tower_service = make_service.call(...).await?;

    // User controls the transformation
    let tower_service = tower_service.map_request(|req: Request<Incoming>| {
        req.map(MyCustomBody::from)
    });

    let hyper_service = TowerToHyperService::new(tower_service);
    tokio::spawn(async move {
        builder.serve_connection(stream, hyper_service).await
    });
}
```

**Pros:** No changes needed, maximum flexibility for power users
**Cons:** `serve()` users are stuck with `Incoming`

#### Option D: Two functions

Offer both:
- `serve()` - Transforms `Incoming` → `Body` (convenient)
- `serve_with_incoming()` - Passes `Incoming` directly (zero-cost)

**Pros:** Clear choice for users
**Cons:** API duplication

### Recommendation

**Option A** (transform to `Body`) is recommended because:
1. Matches Axum's proven approach
2. Decouples users from hyper internals
3. Boxing overhead is negligible (once per request, ~20-50ns)
4. Power users can still write custom loops

---

## 7. Implementation Checklist

### Phase 1: Route Default (This PR)

- [ ] Remove or change default type parameter on `Route<B>`
- [ ] Update any doc comments that rely on the default
- [ ] Verify generated SDKs still compile (they specify types explicitly)
- [ ] Update examples if they rely on the default

### Phase 2: Body Wrapper & serve() Function (Future PR)

- [x] `try_downcast()` already exists in `src/body.rs:61`
- [ ] Add `Body` struct to `src/body.rs`
- [ ] Add `From` implementations for `Body`
- [ ] Export `Body` from `src/lib.rs`
- [ ] Update `serve()` to transform `Incoming` → `Body`
- [ ] Update `serve()` bounds to use `Body`
- [ ] Update `handle_connection()` with `.map_request()`
- [ ] Update examples
- [ ] Verify event stream tests still pass
- [ ] Add unit tests for `Body`

---

## 8. Summary

The `aws-smithy-http-server` routing infrastructure is already well-designed with generic body type parameters throughout. The only coupling to `hyper::body::Incoming` is:

1. **`Route`'s default type parameter** - Cosmetic, should be removed or changed
2. **`serve()` function bounds** - Functional constraint, to be addressed separately

Generated SDKs and middleware are already body-agnostic. No changes needed there.
