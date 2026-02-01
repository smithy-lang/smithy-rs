# Zero-Cost Multi-Protocol Routing Design

## Overview

This document describes the zero-cost abstraction for multi-protocol routing in Smithy services.
The design leverages Rust's type system to eliminate runtime overhead for unused protocol slots
while providing flexibility for users to configure which protocols their service supports.

## Goals

1. **Zero-cost for unused protocols**: When a protocol slot is `()`, the compiler eliminates
   all related branches entirely.
2. **Single route matching**: Avoid calling `match_route()` twice (once in `can_handle()`, once in `call()`).
3. **Configurable fallback**: Users can provide a custom fallback service for unmatched requests.
4. **Type-safe**: All decisions are made at compile time where possible.

## Core Design

### The `ProtocolSlot` Trait

```rust
pub trait ProtocolSlot<B, RespBody, E> {
    /// The future type returned by `call()`.
    type Future: Future<Output = Result<Response<RespBody>, E>>;

    /// The "proof" that we can handle this request.
    /// For cheap protocol detection (header checks), this is `()`.
    /// For expensive detection (route matching), this caches the matched service.
    type Match;

    /// Check if this protocol can handle the given request.
    /// Returns `Some(match_result)` if it can handle it, `None` otherwise.
    fn can_handle(&self, req: &Request<B>) -> Option<Self::Match>;

    /// Handle the request, using the cached match result from `can_handle()`.
    fn call(&mut self, req: Request<B>, matched: Self::Match) -> Self::Future;
}
```

### Implementation for `()` (Unused Slots)

When a protocol slot is `()`, the compiler eliminates the branch entirely:

```rust
impl<B, RespBody, E> ProtocolSlot<B, RespBody, E> for () {
    type Future = std::future::Pending<Result<Response<RespBody>, E>>;
    type Match = Infallible;  // Can never be constructed

    #[inline(always)]
    fn can_handle(&self, _req: &Request<B>) -> Option<Self::Match> {
        None  // Compiler knows Infallible can't exist, optimizes away
    }

    fn call(&mut self, _req: Request<B>, matched: Self::Match) -> Self::Future {
        match matched {}  // Infallible - this is unreachable
    }
}
```

The key insight: `Option<Infallible>` can only ever be `None`, so the compiler eliminates
the entire `if let Some(...)` branch as dead code. Using `Infallible` (an uninhabited type)
instead of `()` gives us a compile-time guarantee that the branch is impossible.

### Implementation for Real Routing Services

Each protocol implements `ProtocolSlot` with an appropriate `Match` type:

```rust
// For header-only protocols (cheap detection), Match = ()
impl<S, B, RespBody, E> ProtocolSlot<B, RespBody, E> for CborRoutingService<S>
where
    CborRoutingService<S>: Service<Request<B>, Response = Response<RespBody>, Error = E>,
    // ...
{
    type Future = <CborRoutingService<S> as Service<Request<B>>>::Future;
    type Match = ();  // Nothing to cache, detection is cheap

    fn can_handle(&self, req: &Request<B>) -> Option<Self::Match> {
        is_rpc_v2_cbor(req).then_some(())
    }

    fn call(&mut self, req: Request<B>, _matched: Self::Match) -> Self::Future {
        Service::call(self, req)
    }
}

// For REST protocols (expensive route matching), Match = S (cached service)
impl<S, B, RespBody, E> ProtocolSlot<B, RespBody, E> for RestJson1RoutingService<S>
where
    RestRouter<S>: Router<B, Service = S>,
    S: Clone + Service<Request<B>, Response = Response<RespBody>, Error = E>,
    // ...
{
    type Future = <S as Service<Request<B>>>::Future;
    type Match = S;  // The matched route/service

    fn can_handle(&self, req: &Request<B>) -> Option<Self::Match> {
        if is_json_content_type(req) {
            self.router().match_route(req).ok()
        } else {
            None
        }
    }

    fn call(&mut self, req: Request<B>, mut matched: Self::Match) -> Self::Future {
        // Use `matched` directly - no need to call match_route() again!
        matched.call(req)
    }
}
```

### Protocol Detection Cost Summary

| Protocol   | Detection Method                              | Expensive? | Match Type |
|------------|-----------------------------------------------|------------|------------|
| RpcV2Cbor  | Header check (`smithy-protocol: rpc-v2-cbor`) | No         | `()`       |
| AwsJson1.1 | Header checks (`x-amz-target` + content-type) | No         | `()`       |
| AwsJson1.0 | Header checks (`x-amz-target` + content-type) | No         | `()`       |
| RestJson1  | Content-type + `match_route()`                | **Yes**    | `S`        |
| RestXml    | Content-type + `match_route()`                | **Yes**    | `S`        |

## The Multi-Protocol Service

```rust
#[derive(Clone, Debug)]
pub struct MultiProtocolService<
    RpcV2 = (),
    AwsJson11 = (),
    AwsJson10 = (),
    RestJson = (),
    RestXml = (),
    Fallback = DefaultNotFoundService,
> {
    rpc_v2: RpcV2,
    aws_json_11: AwsJson11,
    aws_json_10: AwsJson10,
    rest_json: RestJson,
    rest_xml: RestXml,
    fallback: Fallback,
}
```

### Default Fallback Service

```rust
#[derive(Debug, Clone, Copy)]
pub struct DefaultNotFoundService;

impl<B> Service<Request<B>> for DefaultNotFoundService {
    type Response = Response<BoxBody>;
    type Error = Infallible;
    type Future = std::future::Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _req: Request<B>) -> Self::Future {
        std::future::ready(Ok(Response::builder()
            .status(http::StatusCode::NOT_FOUND)
            .header(http::header::CONTENT_TYPE, "application/json")
            .body(crate::body::to_boxed("{}"))
            .expect("valid response")))
    }
}
```

Users can override this with:
- A specific protocol service (e.g., RestXml as the default)
- A custom error handler
- Any `Service<Request<B>>` implementation

## The Future Enum

Since each protocol's `call()` returns a different future type, we need an enum to unify them:

```rust
pin_project_lite::pin_project! {
    #[project = MultiProtocolFutureProj]
    pub enum MultiProtocolFuture<RpcV2Fut, AwsJson11Fut, AwsJson10Fut, RestJsonFut, RestXmlFut, FallbackFut> {
        RpcV2 { #[pin] fut: RpcV2Fut },
        AwsJson11 { #[pin] fut: AwsJson11Fut },
        AwsJson10 { #[pin] fut: AwsJson10Fut },
        RestJson { #[pin] fut: RestJsonFut },
        RestXml { #[pin] fut: RestXmlFut },
        Fallback { #[pin] fut: FallbackFut },
    }
}
```

For `()` slots, the future type is `std::future::Pending<...>` which is zero-sized and
never constructed, so unused variants don't add runtime cost.

## Service Implementation

```rust
impl<B, RpcV2, AwsJson11, AwsJson10, RestJson, RestXml, Fallback, RespBody, E> Service<Request<B>>
    for MultiProtocolService<RpcV2, AwsJson11, AwsJson10, RestJson, RestXml, Fallback>
where
    RpcV2: ProtocolSlot<B, RespBody, E>,
    AwsJson11: ProtocolSlot<B, RespBody, E>,
    AwsJson10: ProtocolSlot<B, RespBody, E>,
    RestJson: ProtocolSlot<B, RespBody, E>,
    RestXml: ProtocolSlot<B, RespBody, E>,
    Fallback: Service<Request<B>, Response = Response<RespBody>, Error = E>,
    Fallback::Future: Future<Output = Result<Response<RespBody>, E>>,
{
    type Response = Response<RespBody>;
    type Error = E;
    type Future = MultiProtocolFuture<...>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request<B>) -> Self::Future {
        // For () slots, compiler sees:
        //   if let Some(m) = None::<Infallible> { ... }
        // → entire branch eliminated

        if let Some(matched) = self.rpc_v2.can_handle(&req) {
            return MultiProtocolFuture::RpcV2 { fut: self.rpc_v2.call(req, matched) };
        }
        if let Some(matched) = self.aws_json_11.can_handle(&req) {
            return MultiProtocolFuture::AwsJson11 { fut: self.aws_json_11.call(req, matched) };
        }
        if let Some(matched) = self.aws_json_10.can_handle(&req) {
            return MultiProtocolFuture::AwsJson10 { fut: self.aws_json_10.call(req, matched) };
        }
        if let Some(matched) = self.rest_json.can_handle(&req) {
            return MultiProtocolFuture::RestJson { fut: self.rest_json.call(req, matched) };
        }
        if let Some(matched) = self.rest_xml.can_handle(&req) {
            return MultiProtocolFuture::RestXml { fut: self.rest_xml.call(req, matched) };
        }

        // Nothing matched - use fallback (always called, no can_handle check)
        MultiProtocolFuture::Fallback { fut: self.fallback.call(req) }
    }
}
```

## Builder Pattern

```rust
impl MultiProtocolService<(), (), (), (), (), DefaultNotFoundService> {
    pub fn new() -> Self {
        Self {
            rpc_v2: (),
            aws_json_11: (),
            aws_json_10: (),
            rest_json: (),
            rest_xml: (),
            fallback: DefaultNotFoundService,
        }
    }
}

impl<RpcV2, AwsJson11, AwsJson10, RestJson, RestXml, Fallback>
    MultiProtocolService<RpcV2, AwsJson11, AwsJson10, RestJson, RestXml, Fallback>
{
    pub fn with_rpc_v2_cbor<S>(
        self,
        svc: CborRoutingService<S>,
    ) -> MultiProtocolService<CborRoutingService<S>, AwsJson11, AwsJson10, RestJson, RestXml, Fallback> {
        MultiProtocolService {
            rpc_v2: svc,
            aws_json_11: self.aws_json_11,
            aws_json_10: self.aws_json_10,
            rest_json: self.rest_json,
            rest_xml: self.rest_xml,
            fallback: self.fallback,
        }
    }

    // Similar for other protocols...

    pub fn fallback<F>(
        self,
        fallback: F,
    ) -> MultiProtocolService<RpcV2, AwsJson11, AwsJson10, RestJson, RestXml, F> {
        MultiProtocolService {
            rpc_v2: self.rpc_v2,
            aws_json_11: self.aws_json_11,
            aws_json_10: self.aws_json_10,
            rest_json: self.rest_json,
            rest_xml: self.rest_xml,
            fallback,
        }
    }
}
```

## Usage Examples

### Single Protocol (RestJson1 only)

```rust
let service = MultiProtocolService::new()
    .with_rest_json1(rest_json_router);

// Type: MultiProtocolService<(), (), (), RestJson1RoutingService<S>, (), DefaultNotFoundService>
// Compiler eliminates all branches except rest_json and fallback
```

### Multiple Protocols

```rust
let service = MultiProtocolService::new()
    .with_rpc_v2_cbor(rpc_v2_router)
    .with_rest_json1(rest_json_router);

// Type: MultiProtocolService<CborRoutingService<S>, (), (), RestJson1RoutingService<S>, (), DefaultNotFoundService>
// Compiler eliminates aws_json_11, aws_json_10, and rest_xml branches
```

### Custom Fallback

```rust
// Use RestXml as fallback for unmatched requests
let service = MultiProtocolService::new()
    .with_rest_json1(rest_json_router)
    .with_rest_xml(rest_xml_router.clone())
    .fallback(rest_xml_router);

// Or with a custom handler:
struct TeapotService;
impl<B> Service<Request<B>> for TeapotService {
    // Returns 418 I'm a teapot
}

let service = MultiProtocolService::new()
    .with_rest_json1(rest_json_router)
    .fallback(TeapotService);
```

## Tests

The implementation includes 12 tests covering:

1. Protocol detection functions (RpcV2, AwsJson1.1, AwsJson1.0, JSON, XML content types)
2. `MultiProtocolService` builder
3. `ProtocolSlot` for `()` always returns `None`
4. `DefaultNotFoundService` poll_ready
5. `Protocol` enum Display
6. `MultiProtocolService` is Clone
7. `MultiProtocolService` is Debug
8. Custom fallback service

## Summary

This design achieves:

1. **Zero-cost abstraction**: Unused protocol slots (`()`) are completely eliminated by the compiler
   thanks to `Infallible` being an uninhabited type
2. **Single route matching**: The `Match` associated type caches expensive route lookups for
   RestJson1 and RestXml protocols
3. **Flexible fallback**: Users can customize what happens when no protocol matches
4. **Type-safe**: All configuration is checked at compile time
5. **Ergonomic builder**: Easy to configure which protocols are enabled