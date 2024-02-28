# Accessing Un-modelled Data

For every [Smithy Operation](https://awslabs.github.io/smithy/2.0/spec/service-types.html#operation) an input, output, and optional error are specified. This in turn constrains the function signature of the handler provided to the service builder - the input to the handler must be the input specified by the operation etc.

But what if we, the customer, want to access data in the handler which is _not_ modelled by our Smithy model? Smithy Rust provides an escape hatch in the form of the `FromParts` trait. In [`axum`](https://docs.rs/axum/latest/axum/index.html) these are referred to as ["extractors"](https://docs.rs/axum/latest/axum/extract/index.html).

<!-- TODO(IntoParts): Dually, what if we want to return data from the handler which is _not_ modelled by our Smithy model? -->

```rust,ignore
/// Provides a protocol aware extraction from a [`Request`]. This borrows the
/// [`Parts`], in contrast to [`FromRequest`].
pub trait FromParts<Protocol>: Sized {
    /// The type of the failures yielded extraction attempts.
    type Rejection: IntoResponse<Protocol>;

    /// Extracts `self` from a [`Parts`] synchronously.
    fn from_parts(parts: &mut Parts) -> Result<Self, Self::Rejection>;
}
```

Here [`Parts`](https://docs.rs/http/latest/http/request/struct.Parts.html) is the struct containing all items in a [`http::Request`](https://docs.rs/http/latest/http/request/struct.Request.html) except for the HTTP body.

A prolific example of a `FromParts` implementation is `Extension<T>`:

```rust,ignore
/// Generic extension type stored in and extracted from [request extensions].
///
/// This is commonly used to share state across handlers.
///
/// If the extension is missing it will reject the request with a `500 Internal
/// Server Error` response.
///
/// [request extensions]: https://docs.rs/http/latest/http/struct.Extensions.html
#[derive(Debug, Clone)]
pub struct Extension<T>(pub T);

/// The extension has not been added to the [`Request`](http::Request) or has been previously removed.
#[derive(Debug, Error)]
#[error("the `Extension` is not present in the `http::Request`")]
pub struct MissingExtension;

impl<Protocol> IntoResponse<Protocol> for MissingExtension {
    fn into_response(self) -> http::Response<BoxBody> {
        let mut response = http::Response::new(empty());
        *response.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
        response
    }
}

impl<Protocol, T> FromParts<Protocol> for Extension<T>
where
    T: Send + Sync + 'static,
{
    type Rejection = MissingExtension;

    fn from_parts(parts: &mut http::request::Parts) -> Result<Self, Self::Rejection> {
        parts.extensions.remove::<T>().map(Extension).ok_or(MissingExtension)
    }
}
```

This allows the service builder to accept the following handler

```rust,ignore
async fn handler(input: ModelInput, extension: Extension<SomeStruct>) -> ModelOutput {
    /* ... */
}
```

where `ModelInput` and `ModelOutput` are specified by the Smithy Operation and `SomeStruct` is a struct which has been inserted, by middleware, into the [`http::Request::extensions`](https://docs.rs/http/latest/http/request/struct.Request.html#method.extensions).

Up to 32 structures implementing `FromParts` can be provided to the handler with the constraint that they _must_ be provided _after_ the `ModelInput`:

```rust,ignore
async fn handler(input: ModelInput, ext1: Extension<SomeStruct1>, ext2: Extension<SomeStruct2>, other: Other /* : FromParts */, /* ... */) -> ModelOutput {
    /* ... */
}
```

Note that the `parts.extensions.remove::<T>()` in `Extensions::from_parts` will cause multiple `Extension<SomeStruct>` arguments in the handler to fail. The first extraction failure to occur is serialized via the `IntoResponse` trait (notice `type Error: IntoResponse<Protocol>`) and returned.

The `FromParts` trait is public so customers have the ability specify their own implementations:

```rust,ignore
struct CustomerDefined {
    /* ... */
}

impl<P> FromParts<P> for CustomerDefined {
    type Error = /* ... */;

    fn from_parts(parts: &mut Parts) -> Result<Self, Self::Error> {
        // Construct `CustomerDefined` using the request headers.
        let header_value = parts.headers.get("header-name").ok_or(/* ... */)?;
        Ok(CustomerDefined { /* ... */ })
    }
}

async fn handler(input: ModelInput, arg: CustomerDefined) -> ModelOutput {
    /* ... */
}
```
