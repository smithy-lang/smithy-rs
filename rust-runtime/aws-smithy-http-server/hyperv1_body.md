# Hyper 1.0 Body Design Decision Research

## Why Hyper 1.0 Removed the Body Struct

In Hyper v1.0, the `hyper::Body` changed from a concrete struct to a trait. This was a significant architectural decision with clear reasoning behind it.

## Key Reasoning

### 1. **Forward Compatibility**
- **Primary motivation**: Enable adding support for new HTTP features without requiring major version bumps
- **Future-proofing**: Allow the library to evolve with emerging HTTP standards
- **API stability**: Maintain backwards compatibility while adding new capabilities

### 2. **HTTP/2 Feature Support**
The existing Stream-based Body struct prevented implementing important HTTP/2 features:

- **HTTP Trailers**: Rarely used in HTTP/1.1 but common in HTTP/2 (especially gRPC)
- **Push Promises**: HTTP/2 server push functionality  
- **Flow Control**: Advanced HTTP/2 flow control mechanisms
- **Future Frame Types**: Extensibility for new HTTP/2+ frame types

### 3. **Design Philosophy Shift**
- **From concrete to abstract**: Move from opinionated implementations to flexible traits
- **Ecosystem approach**: Let higher-level libraries (like Axum, Reqwest) provide opinionated implementations
- **Core library focus**: Keep Hyper lean and focused on low-level HTTP primitives

## Technical Impact

### Before (Hyper 0.x):
```rust
hyper::Body  // Concrete struct
```

### After (Hyper 1.x):
```rust
// Trait-based approach
trait Body {
    type Data;
    type Error;
    // ...
}

// Specific implementations:
hyper::body::Bytes      // Simple bytes body
hyper::body::Incoming   // Streaming body from connections
http_body_util::Full    // Complete body in memory  
http_body_util::Empty   // Empty body
http_body_util::combinators::BoxBody  // Type-erased body
```

## Ecosystem Response

### Axum's Solution
Axum created `axum_core::body::Body` because:
- **Stability**: Avoid depending on unstable `hyper-util` crate
- **Ergonomics**: Provide a convenient body type for application developers  
- **Internal consistency**: Have a single body type for internal APIs

Axum's approach:
```rust
// Axum's body wrapper
pub struct Body(/* internal boxed body */);

impl Body {
    pub fn new<B>(body: B) -> Self 
    where 
        B: http_body::Body<Data = Bytes> + Send + 'static,
        B::Error: Into<BoxError>,
    {
        // Box the body for type erasure
    }
}
```

## Implications for Our Implementation

### Why This Matters for smithy-rs:
1. **No built-in Body type**: We need to choose/create our own body abstraction
2. **Type erasure needed**: Similar to Axum's approach for API consistency  
3. **Feature compatibility**: Must work with both HTTP/1.1 and HTTP/2 features
4. **Ecosystem alignment**: Should follow patterns established by major frameworks

## References
- [Hyper v1 Blog Post](https://seanmonstar.com/blog/hyper-v1/)
- [Axum Body PR](https://github.com/tokio-rs/axum/pull/1584)  
- [Hyper Body Trait Issue](https://github.com/hyperium/hyper/issues/1438)
- [Split Body Type Issue](https://github.com/hyperium/hyper/issues/2345)
