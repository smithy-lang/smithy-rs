# Instructions: Update Generated Pokemon Service SDK

## Background

We have implemented a new zero-cost `MultiProtocolService` in this file:
- Design document: `aws-smithy-http-server/src/routing/new-design.md`
- Implementation: `aws-smithy-http-server/src/routing/multi_protocol.rs`

The generated Pokemon service SDK needs to be updated to use our new types.

## Files to Modify

- **Generated SDK:** `/Volumes/workplace/multi-protocol/examples/pokemon-service-server-sdk/src/service.rs`

## Public Type Aliases Available

The following type aliases are now public and exported from `aws_smithy_http_server::routing`:

```rust
pub type CborRoutingService<S> = RoutingService<RpcV2CborRouter<S>, RpcV2Cbor>;
pub type AwsJson11RoutingService<S> = RoutingService<AwsJsonRouter<S>, AwsJson1_1>;
pub type AwsJson10RoutingService<S> = RoutingService<AwsJsonRouter<S>, AwsJson1_0>;
pub type RestJson1RoutingService<S> = RoutingService<RestRouter<S>, RestJson1>;
pub type RestXmlRoutingService<S> = RoutingService<RestRouter<S>, RestXml>;
pub struct DefaultNotFoundService;
```

## Instructions

### Step 1: Rename Type

Change all occurrences of:
```
MultiProtocolRoutingService
```
To:
```
MultiProtocolService
```

There are 10 occurrences in service.rs.

### Step 2: Update Generic Parameter Types

**Old pattern (raw router types):**
```rust
::aws_smithy_http_server::routing::MultiProtocolRoutingService<
    ::aws_smithy_http_server::protocol::rpc_v2_cbor::router::RpcV2CborRouter<S>,
    (),
    (),
    ::aws_smithy_http_server::protocol::rest::router::RestRouter<S>,
    (),
>
```

**New pattern (using public type aliases):**
```rust
::aws_smithy_http_server::routing::MultiProtocolService<
    ::aws_smithy_http_server::routing::CborRoutingService<S>,
    (),
    (),
    ::aws_smithy_http_server::routing::RestJson1RoutingService<S>,
    (),
    ::aws_smithy_http_server::routing::DefaultNotFoundService,
>
```

### Step 3: Add 6th Generic Parameter

The new design has 6 generic parameters (added `Fallback`). Add `DefaultNotFoundService` as the 6th parameter in all type declarations.

### Step 4: Verify Generic Parameter Order

New order (matches protocol detection priority - most specific first):
1. `RpcV2` - `CborRoutingService<S>` or `()`
2. `AwsJson11` - `AwsJson11RoutingService<S>` or `()`
3. `AwsJson10` - `AwsJson10RoutingService<S>` or `()`
4. `RestJson` - `RestJson1RoutingService<S>` or `()`
5. `RestXml` - `RestXmlRoutingService<S>` or `()`
6. `Fallback` - `DefaultNotFoundService`

### Locations to Update in service.rs

| Line | Description |
|------|-------------|
| 14 | `PokemonServiceRouter` struct field `inner` type |
| 43 | `Deref::Target` type |
| 64 | `Service` impl where clause |
| 72 | `Service::Response` associated type |
| 80 | `Service::Error` associated type |
| 88 | `Service::Future` associated type |
| 1388 | Constructor call in `try_build()` |
| 1574 | Constructor call in `build_unchecked()` |

### Step 5: Verify Builder Calls

The builder method names are unchanged:
- `.with_rpc_v2_cbor(svc)`
- `.with_rest_json1(svc)`

The generated code already creates `RoutingService::new(router)` before calling these methods.
This should work with our new design. No changes needed here.

### Step 6: Test

After making changes, run:
```bash
cd /Volumes/workplace/multi-protocol/examples
cargo build --package pokemon-service-server-sdk
```

## Example: Complete Type Declaration

For `PokemonServiceRouter` which uses RpcV2Cbor and RestJson1:

```rust
pub struct PokemonServiceRouter<
    S = ::aws_smithy_http_server::routing::Route<::aws_smithy_http_server::body::BoxBody>,
> {
    inner: ::aws_smithy_http_server::routing::MultiProtocolService<
        ::aws_smithy_http_server::routing::CborRoutingService<S>,
        (),
        (),
        ::aws_smithy_http_server::routing::RestJson1RoutingService<S>,
        (),
        ::aws_smithy_http_server::routing::DefaultNotFoundService,
    >,
}
```

---

## Future Improvement: Type Alias Ergonomics for Customers

### Problem

Some customers using `build_unchecked()` currently have clean, readable return types:

```rust
async fn create_router(
    args: &ServiceArguments,
    enable_appconfig: bool,
) -> Result<VerifiedPermissions<RoutingService<AwsJsonRouter<Route>, AwsJson1_0>>, Box<dyn Error>> {
    // ...
    Ok(VerifiedPermissions::builder(config)
        ./* handlers */
        .build_unchecked())
}
```

With multi-protocol support, they would have to write the full `MultiProtocolService` type:

```rust
Result<VerifiedPermissions<MultiProtocolService<
    (),
    AwsJson10RoutingService<Route>,
    (),
    (),
    (),
    DefaultNotFoundService,
>>, Box<dyn Error>>
```

This is verbose and error-prone.

### Recommended Solution: Generate a Type Alias

The code generator should emit a type alias for each service's router type. Since the generator
already knows exactly which protocols the service uses, it can produce a clean alias:

```rust
// In generated service.rs
pub type VerifiedPermissionsRouter = MultiProtocolService<
    (),
    AwsJson10RoutingService<Route>,
    (),
    (),
    (),
    DefaultNotFoundService,
>;
```

Customers then write:

```rust
async fn create_router(
    args: &ServiceArguments,
    enable_appconfig: bool,
) -> Result<VerifiedPermissions<VerifiedPermissionsRouter>, Box<dyn Error>> {
    // ...
}
```

### Alternative Solutions Considered

1. **`impl Service` in return position** - Loses type information, may not work for all use cases
2. **Type inference** - Works for local variables but not function signatures
3. **Wrapper struct** - This is what `PokemonServiceRouter` already does; the type alias approach
   is simpler and achieves the same ergonomics

### Action Item

Update the Smithy code generator to emit a `<ServiceName>Router` type alias alongside the
existing `<ServiceName>Router` struct, or ensure customers know to use the struct name
(e.g., `PokemonServiceRouter`) instead of the raw `MultiProtocolService` type.
