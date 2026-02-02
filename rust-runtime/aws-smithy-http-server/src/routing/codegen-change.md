# Code Generator Changes Required for MultiProtocolService

This document outlines the changes required in the Smithy code generator (codegen-server) to produce code compatible with the new `MultiProtocolService` design.

## Overview

The generated Pokemon service SDK was manually updated to use the new `MultiProtocolService` type. These changes need to be implemented in the code generator so that all generated SDKs will produce the correct code.

## Files to Modify in codegen-server

The primary file to modify is:
- `codegen-server/src/main/kotlin/software/amazon/smithy/rust/codegen/server/smithy/generators/ServerServiceGenerator.kt`

Additional files that may need updates:
- `codegen-server/src/main/kotlin/software/amazon/smithy/rust/codegen/server/smithy/generators/ServerRootGenerator.kt`

## Change 1: Replace Struct Wrapper with Type Alias

### Location
`ServerServiceGenerator.kt` - where `{ServiceName}Router` is generated

### Change
Replace the struct wrapper with a simple type alias. This eliminates ~80 lines of boilerplate code (Clone, Debug, Deref, DerefMut, Service impls).

### Before (Current Generated Code)
```rust
pub struct PokemonServiceRouter<
    S = ::aws_smithy_http_server::routing::Route<::aws_smithy_http_server::body::BoxBody>,
> {
    inner: ::aws_smithy_http_server::routing::MultiProtocolRoutingService<
        ::aws_smithy_http_server::protocol::rpc_v2_cbor::router::RpcV2CborRouter<S>,
        (),
        (),
        ::aws_smithy_http_server::protocol::rest::router::RestRouter<S>,
        (),
    >,
}

impl<S> Clone for PokemonServiceRouter<S>
where
    S: Clone,
{
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<S: std::fmt::Debug> std::fmt::Debug for PokemonServiceRouter<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PokemonServiceRouter")
            .field("inner", &self.inner)
            .finish()
    }
}

impl<S> std::ops::Deref for PokemonServiceRouter<S> {
    type Target = /* ... */;
    fn deref(&self) -> &Self::Target { &self.inner }
}

impl<S> std::ops::DerefMut for PokemonServiceRouter<S> {
    fn deref_mut(&mut self) -> &mut Self::Target { &mut self.inner }
}

impl<S, Body> ::tower::Service<::http_1x::Request<Body>> for PokemonServiceRouter<S>
where
    /* ... complex bounds ... */
{
    type Response = /* ... */;
    type Error = /* ... */;
    type Future = /* ... */;
    
    fn poll_ready(&mut self, cx: &mut std::task::Context<'_>) -> std::task::Poll<Result<(), Self::Error>> {
        ::tower::Service::poll_ready(&mut self.inner, cx)
    }

    fn call(&mut self, req: ::http_1x::Request<Body>) -> Self::Future {
        ::tower::Service::call(&mut self.inner, req)
    }
}
```

### After (New Generated Code)
```rust
/// Type alias for the multi-protocol router used by this service.
/// 
/// This type handles routing requests to the appropriate protocol handler
/// based on request characteristics (headers, content-type, URI path).
/// 
/// The type parameter `B` is the request body type, defaulting to
/// `hyper::body::Incoming` for standard HTTP server use cases.
pub type PokemonServiceRouter<B = ::hyper::body::Incoming> =
    ::aws_smithy_http_server::routing::MultiProtocolService<
        ::aws_smithy_http_server::routing::CborRoutingService<
            ::aws_smithy_http_server::routing::Route<B>,
        >,
        (),
        (),
        ::aws_smithy_http_server::routing::RestJson1RoutingService<
            ::aws_smithy_http_server::routing::Route<B>,
        >,
        (),
        ::aws_smithy_http_server::routing::DefaultNotFoundService,
    >;
```

### Benefits
- **No boilerplate** - `Clone`, `Debug`, `Deref`, `DerefMut`, `Service` all come from `MultiProtocolService`
- **Correct default** - uses `hyper::body::Incoming` for request body (not `BoxBody`)
- **Transparent** - all traits automatically available
- **Simple for customers** - just write `PokemonService<PokemonServiceRouter>`

## Change 2: Rename `MultiProtocolRoutingService` to `MultiProtocolService`

### Location
`ServerServiceGenerator.kt` - all references to `MultiProtocolRoutingService`

### Change
Replace all occurrences of:
```kotlin
MultiProtocolRoutingService
```
With:
```kotlin
MultiProtocolService
```

## Change 3: Use Public Type Aliases for Protocol Routers

### Location
`ServerServiceGenerator.kt` - where the router types are generated

### Change
Instead of generating raw router types, use the public type aliases from `aws_smithy_http_server::routing`.

### Mapping

| Protocol | Old Type (Raw Router) | New Type (Public Alias) |
|----------|----------------------|------------------------|
| RpcV2Cbor | `::aws_smithy_http_server::protocol::rpc_v2_cbor::router::RpcV2CborRouter<S>` | `::aws_smithy_http_server::routing::CborRoutingService<Route<B>>` |
| AwsJson1.1 | `::aws_smithy_http_server::protocol::aws_json_11::router::AwsJsonRouter<S>` | `::aws_smithy_http_server::routing::AwsJson11RoutingService<Route<B>>` |
| AwsJson1.0 | `::aws_smithy_http_server::protocol::aws_json_10::router::AwsJsonRouter<S>` | `::aws_smithy_http_server::routing::AwsJson10RoutingService<Route<B>>` |
| RestJson1 | `::aws_smithy_http_server::protocol::rest::router::RestRouter<S>` | `::aws_smithy_http_server::routing::RestJson1RoutingService<Route<B>>` |
| RestXml | `::aws_smithy_http_server::protocol::rest::router::RestRouter<S>` | `::aws_smithy_http_server::routing::RestXmlRoutingService<Route<B>>` |

## Change 4: Add 6th Generic Parameter (DefaultNotFoundService)

### Location
`ServerServiceGenerator.kt` - where the `MultiProtocolService` type is generated

### Change
Add `::aws_smithy_http_server::routing::DefaultNotFoundService` as the 6th generic parameter.

### Generic Parameter Order
The order (matching protocol detection priority):
1. `RpcV2` - `CborRoutingService<Route<B>>` or `()`
2. `AwsJson11` - `AwsJson11RoutingService<Route<B>>` or `()`
3. `AwsJson10` - `AwsJson10RoutingService<Route<B>>` or `()`
4. `RestJson` - `RestJson1RoutingService<Route<B>>` or `()`
5. `RestXml` - `RestXmlRoutingService<Route<B>>` or `()`
6. `Fallback` - `DefaultNotFoundService`

## Change 5: Export Router Type Alias

### Location
`ServerRootGenerator.kt` - where public exports are generated in `lib.rs`

### Change
Add `{ServiceName}Router` to the public exports:

```rust
// Before
pub use crate::service::{
    MissingOperationsError, PokemonService, PokemonServiceBuilder, PokemonServiceConfig,
    PokemonServiceConfigBuilder,
};

// After
pub use crate::service::{
    MissingOperationsError, PokemonService, PokemonServiceBuilder, PokemonServiceConfig,
    PokemonServiceConfigBuilder, PokemonServiceRouter,
};
```

This enables customers to write clean type signatures:
```rust
async fn create_router() -> Result<PokemonService<PokemonServiceRouter>, Box<dyn Error>> {
    // ...
}
```

## Change 6: Update Builder Methods

### Location
`ServerServiceGenerator.kt` - `build()` and `build_unchecked()` methods

### Change
The builder should construct the `MultiProtocolService` directly (no struct wrapper):

```rust
// In build() and build_unchecked()
let inner = ::aws_smithy_http_server::routing::MultiProtocolService::new()
    .with_rest_json1(rest_json_1_svc)
    .with_rpc_v2_cbor(rpc_v2_cbor_svc);

// Return type is now the type alias directly
PokemonService { svc: inner }
```

## Change 7: Remove Doc Test Type Annotations

### Location
`ServerServiceGenerator.kt` - doc comment generation for builder methods

### Change
The hidden type annotation lines in doc tests can be simplified since the type alias is now public:

```rust
// Before (problematic - referenced private module)
/// # let app: PokemonService<crate::service::PokemonServiceRouter> = app;

// After (simple - type alias is public)
/// # let _: PokemonService<PokemonServiceRouter> = app;
```

Or just remove the type annotation entirely:
```rust
/// # drop(app);
```

## Summary of All Changes

| # | Change | File | Description |
|---|--------|------|-------------|
| 1 | Use type alias | ServerServiceGenerator.kt | Replace struct wrapper with `pub type {Service}Router<B = Incoming> = MultiProtocolService<...>` |
| 2 | Rename type | ServerServiceGenerator.kt | `MultiProtocolRoutingService` → `MultiProtocolService` |
| 3 | Use type aliases | ServerServiceGenerator.kt | Raw router types → Public routing type aliases |
| 4 | Add 6th parameter | ServerServiceGenerator.kt | Add `DefaultNotFoundService` as fallback |
| 5 | Export router | ServerRootGenerator.kt | Add `{ServiceName}Router` to public exports |
| 6 | Update builder | ServerServiceGenerator.kt | Construct `MultiProtocolService` directly |
| 7 | Fix doc tests | ServerServiceGenerator.kt | Update/remove type annotations |

## Testing

After implementing these changes, verify by:

1. Running codegen: `./gradlew codegen-server-test:assemble`
2. Comparing generated output with the expected type alias format
3. Running tests: `cd examples && cargo test`

## Example: Complete Generated Type Alias

For a service named `PokemonService` using RpcV2Cbor and RestJson1:

```rust
/// Type alias for the multi-protocol router used by this service.
pub type PokemonServiceRouter<B = ::hyper::body::Incoming> =
    ::aws_smithy_http_server::routing::MultiProtocolService<
        ::aws_smithy_http_server::routing::CborRoutingService<
            ::aws_smithy_http_server::routing::Route<B>,
        >,
        (),
        (),
        ::aws_smithy_http_server::routing::RestJson1RoutingService<
            ::aws_smithy_http_server::routing::Route<B>,
        >,
        (),
        ::aws_smithy_http_server::routing::DefaultNotFoundService,
    >;
```

## Customer Usage

With these changes, customers can write:

```rust
use pokemon_service_server_sdk::{PokemonService, PokemonServiceRouter};

async fn create_router(
    args: &ServiceArguments,
) -> Result<PokemonService<PokemonServiceRouter>, Box<dyn Error>> {
    let config = PokemonServiceConfig::builder().build();
    
    Ok(PokemonService::builder(config)
        .check_health(handlers::check_health)
        .get_pokemon_species(handlers::get_pokemon_species)
        // ... other handlers
        .build_unchecked())
}
```

No need to spell out complex generic parameters!