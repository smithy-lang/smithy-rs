# Error Conversions in Request Rejection

This document explains why we need **two different** error conversions for `RequestRejection` in each protocol.

## TL;DR

The framework needs to handle errors from **two different body types**:

1. **`hyper::body::Incoming`** (production) → errors are `hyper::Error`
2. **`BoxBody`** (tests/custom) → errors are `crate::Error`

Therefore, each protocol's `RequestRejection` implements:
```rust
impl From<hyper::Error> for RequestRejection { ... }
impl From<crate::Error> for RequestRejection { ... }
```

## The Two Body Types

### 1. Production: `hyper::body::Incoming`

**Location**: Real HTTP requests from Hyper server

```rust
// In production, Hyper provides:
Request<hyper::body::Incoming>

// Where:
impl HttpBody for hyper::body::Incoming {
    type Error = hyper::Error;  // ← Production error type
    // ...
}
```

**When errors occur**:
```rust
// In FromRequest implementation (generated code):
let bytes = body.collect().await?.to_bytes();
//                           ^^^ hyper::Error here!
```

### 2. Testing/Custom: `BoxBody`

**Location**: Protocol tests, mocked requests, generic body parameters

**Definition** (`src/body.rs:26`):
```rust
pub type BoxBody = http_body_util::combinators::UnsyncBoxBody<Bytes, Error>;
//                                                                     ^^^^^
//                                                                 crate::Error
```

**When errors occur**:
```rust
// In tests or when using BoxBody:
Request<BoxBody>

// Where:
impl HttpBody for BoxBody {
    type Error = crate::Error;  // ← Test/custom error type
    // ...
}

// Reading body:
let bytes = body.collect().await?.to_bytes();
//                           ^^^ crate::Error here!
```

## Real Example: Test Failure

When we commented out `From<crate::Error>`, tests failed with:

```
error[E0277]: the trait bound `rest_json_1::rejection::RequestRejection:
From<aws_smithy_http_server::error::Error>` is not satisfied
  --> rest_json/rust-server-codegen/src/operation.rs:48734:5
   |
   | impl Service<http::Request<UnsyncBoxBody<Bytes, Error>>>
   |                            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
   |                            This is BoxBody, not Incoming!
```

The test was using `Request<BoxBody>`, so body errors were `crate::Error`, not `hyper::Error`.

## Why Not Just Use One?

### Can't Remove `From<hyper::Error>`

Production code **must** handle Hyper's error type:

```rust
// Production: Hyper server
serve(listener, app.into_make_service()).await?;
//                ^
//                └─ Provides Request<hyper::body::Incoming>

// Handler receives:
async fn handler(input: Input) -> Result<Output, Error> {
    // Input was deserialized from hyper::body::Incoming
    // If body.collect() fails → hyper::Error → needs From<hyper::Error>
}
```

### Can't Remove `From<crate::Error>`

Tests and custom body types **must** handle `crate::Error`:

```rust
// In protocol tests:
let body = BoxBody::new(...);  // Error type: crate::Error
let request = Request::new(body);

// When body.collect() fails → crate::Error → needs From<crate::Error>
```

### Why Not Chain Conversions?

You might think: "Can't we do `hyper::Error → crate::Error → RequestRejection`?"

**No!** Rust's `?` operator only looks for **direct** `From` implementations. It doesn't chain:

```rust
// This doesn't work:
body.collect().await?
// ^^^ Looks for From<hyper::Error> for RequestRejection
//     Does NOT look for From<hyper::Error> for crate::Error
//              then From<crate::Error> for RequestRejection
```

## Summary of Both Conversions

### `From<hyper::Error>` for RequestRejection

**Purpose**: Handle errors from production HTTP requests

**Used by**: Generated `FromRequest` implementations when body type is `hyper::body::Incoming`

**Comment**:
```rust
// Hyper's HTTP server provides requests with `hyper::body::Incoming`, which has error type
// `hyper::Error`. During request deserialization (FromRequest), body operations can produce
// this error, so we need this conversion to handle it within the framework.
convert_to_request_rejection!(hyper::Error, BufferHttpBodyBytes);
```

### `From<crate::Error>` for RequestRejection

**Purpose**: Handle errors from tests and custom body types

**Used by**:
- Protocol tests using `BoxBody`
- Custom body implementations that use `crate::Error`
- Generic code where body type `B` is constrained to `B::Error: Into<crate::Error>`

**Comment**:
```rust
// Conversion from crate::Error is needed for custom body types and testing scenarios.
// When using BoxBody or custom body implementations, errors are crate::Error, not hyper::Error.
// This is commonly used in protocol tests and when body types are wrapped/transformed.
impl From<crate::Error> for RequestRejection {
    fn from(err: crate::Error) -> Self {
        Self::BufferHttpBodyBytes(err)
    }
}
```

## Code Locations

All four protocols have both conversions:

| Protocol | File | Lines |
|----------|------|-------|
| RestJson1 | `src/protocol/rest_json_1/rejection.rs` | 195-198 (crate::Error), 214 (hyper::Error) |
| RestXml | `src/protocol/rest_xml/rejection.rs` | 77-80 (crate::Error), 90 (hyper::Error) |
| AwsJson | `src/protocol/aws_json/rejection.rs` | 47-50 (crate::Error), 55 (hyper::Error) |
| RpcV2Cbor | `src/protocol/rpc_v2_cbor/rejection.rs` | 51-54 (crate::Error), 58 (hyper::Error) |

## Testing the Necessity

We verified both are needed by:

1. **Commented out** `From<crate::Error>` implementations
2. **Ran** `./gradlew codegen-server-test:test`
3. **Got errors** like:
   ```
   error[E0277]: the trait bound `RequestRejection: From<crate::Error>` is not satisfied
     --> src/operation.rs:48734:5
      |
      | impl Service<http::Request<UnsyncBoxBody<Bytes, Error>>>
   ```
4. **Conclusion**: Both conversions are essential for the framework to work with both production (Hyper) and test (BoxBody) scenarios.

## Design Rationale

This dual conversion pattern provides:

✅ **Production Support**: Handles real HTTP requests from Hyper servers
✅ **Test Support**: Enables protocol tests with `BoxBody`
✅ **Extensibility**: Allows custom body implementations
✅ **Type Safety**: Compile-time verification of error handling

The small amount of code duplication (2 conversions per protocol) is worth the flexibility and correctness guarantees it provides.
