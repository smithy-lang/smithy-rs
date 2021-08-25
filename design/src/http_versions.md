RFC: Supporting multiple HTTP versions for SDKs that use Event Stream
=====================================================================

> Status: RFC. For a summarized list of proposed changes, see the [Changes Checklist](#changes-checklist) section.

Most AWS SDK operations use HTTP/1.1, but bi-directional streaming operations that use the Event Stream
message framing format need to use HTTP/2 (h2).

Smithy models can also customize which HTTP versions are used in each individual protocol trait.
For example,
[`@restJson1` has attributes `http` and `eventStreamHttp`](https://awslabs.github.io/smithy/1.0/spec/aws/aws-restjson1-protocol.html#aws-protocols-restjson1-trait)
to list out the versions that should be used in a priority order.

There are two problems in play that this doc attempts to solve:
1. **Connector Creation**: Customers need to be able to create connectors with the HTTP settings they desire,
   and these custom connectors must align with what the Smithy model requires.
2. **Connector Selection**: The generated code must be able to select the connector that best matches the requirements
   from the Smithy model.

Terminology
-----------

Today, there are three layers of `Client` that are easy to confuse, so to make the following easier to follow,
the following terms will be used:

- **Connector**: An implementor of Tower's `Service` trait that converts a request into a response. This is typically
  a thin wrapper around a Hyper client.
- **Smithy Client**: A `smithy_client::Client<C, M, R>` struct that is responsible for gluing together
  the connector, middleware, and retry policy. This isn't intended to be used directly.
- **Fluent Client**: A code generated `Client<C, M, R>` that has methods for each service operation on it.
  A fluent builder is generated alongside it to make construction easier.
- **AWS Client**: A specialized Fluent Client that defaults to using a `DynConnector`, `AwsMiddleware`,
  and `Standard` retry policy.

All of these are just called `Client` in code today. This is something that could be clarified in a separate refactor.

How Clients Work Today
----------------------

Fluent clients currently keep a handle to a single Smithy client, which is a wrapper
around the underlying connector. When constructing operation builders, this handle is `Arc` cloned and
given to the new builder instances so that their `send()` calls can initiate a request.

The generated fluent client code ends up looking like this:
```rust
struct Handle<C, M, R> {
    client: smithy_client::Client<C, M, R>,
    conf: crate::Config,
}

pub struct Client<C, M, R = Standard> {
    handle: Arc<Handle<C, M, R>>,
}
```

Functions are generated per operation on the fluent client to gain access to the individual operation builders.
For example:

```rust
pub fn assume_role(&self) -> fluent_builders::AssumeRole<C, M, R> {
    fluent_builders::AssumeRole::new(self.handle.clone())
}
```

The fluent operation builders ultimately implement `send()`, which chooses the one and only Smithy client out
of the handle to make the request with:

```rust
pub struct AssumeRole<C, M, R> {
    handle: std::sync::Arc<super::Handle<C, M, R>>,
    inner: crate::input::assume_role_input::Builder,
}

impl<C, M, R> AssumeRole<C, M, R> where ...{
    pub async fn send(self) -> Result<AssumeRoleOutput, SdkError<AssumeRoleError>> where ... {
        // Setup code omitted ...

        // Make the actual request
        self.handle.client.call(op).await
    }
}
```

Smithy clients are constructed from a connector, as shown:

```rust
let connector = Builder::new()
    .https()
    .middleware(...)
    .build();
let client = Client::with_config(connector, Config::builder().build());
```

The `https()` method on the Builder constructs the actual Hyper client, and is driven off Cargo features to
select the correct TLS implementation. For example:

```rust
#[cfg(feature = "rustls")]
pub fn https() -> Https {
    let https = hyper_rustls::HttpsConnector::with_native_roots();
    let client = hyper::Client::builder().build::<_, SdkBody>(https);
    // HyperAdapter is a Tower `Service` request -> response connector that just calls the Hyper client
    crate::hyper_impls::HyperAdapter::from(client)
}
```

Solving the Connector Creation Problem
--------------------------------------

Customers need to be able to provide HTTP settings, such as timeouts, for all connectors that the clients use.
These should come out of the `SharedConfig` when it is used. Connector creation also needs to be customizable
so that alternate HTTP implementations can be used, or so that a fake implementation can be used for tests.

To accomplish this, `SharedConfig` will have a `connector_fn` member. A customer would configure
it as such:

```rust
let config = some_shared_config_loader()
    .with_http_settings(my_http_settings)
    .with_connector_fn(|settings: &HttpSettings, http_version: HttpVersion| {
        Some(MyCustomConnector::new(settings, http_version))
    })
    .await;
```

A default `connector_fn` would be provided that creates a Hyper connector based on the Cargo feature flags.
This might look something like this:

```rust
#[cfg(feature = "rustls")]
pub fn default_connector(settings: &HttpSettings, http_version: HttpVersion) -> HyperAdapter {
    let https = hyper_rustls::HttpsConnector::with_native_roots();
    let mut builder = hyper::Client::builder();
    builder = configure_settings(builder, settings);
    if let Http2 = http_version {
        builder = builder.http2_only(true);
    }
    HyperAdapter::from(builder.build::<_, SdkBody>(https))
}
```

For any given service, `connector_fn` could be called multiple times to create connectors
for all required HTTP versions.

**Note:** the `connector_fn` returns an `Option` since an HTTP version may not be required, but rather, preferred
according to a Smithy model. For operations that list out `["h2", "HTTP/1.1"]` as the desired versions,
a customer could choose to provide only an HTTP 1 connector, and the operation should still succeed.

Solving the Connector Selection Problem
---------------------------------------

Each service operation needs to be able to select a connector that meets its requirements best
from the customer provided connectors. As of now, the only selection criteria is the HTTP version.
Since connector creation is not a cheap process, connectors will need to be cached after they are
created, and shared between successive service calls.

This caching is currently handled by the `Handle` in the fluent client, which holds on to the
Smithy client.

To add connector selection, the `Handle` will be adjusted to hold multiple, annotated, Smithy clients:

```rust
enum ConnectorHttpVersion<C, M, R> {
    Http1_1(smithy_client::Client<C, M, R>),
    Http2(smithy_client::Client<C, M, R>),
}

struct Handle<C, M, R> {
    clients: Vec<ConnectorHttpVersion<C, M, R>>,
    conf: crate::Config,
}

pub struct Client<C, M, R = Standard> {
    handle: Arc<Handle<C, M, R>>,
}
```

This arrangement will require that the connector type be the same between HTTP implementations,
but this should be fine since it is generally a thin wrapper around a separate HTTP implementor.
For cases where it is not, the custom connector type can host its own dyn Trait solution.

When generating the fluent client, the code generator knows which HTTP version is preferred for each operation.
So when it generates the `send()` method, it can list out a prioritized list of `HttpVersion`:

```rust
impl<C, M, R> AssumeRole<C, M, R> where ...{
    pub async fn send(self) -> Result<AssumeRoleOutput, SdkError<AssumeRoleError>> where ... {
        // Setup code omitted ...

        // Make the actual request
        self.handle.select_client(&[HttpVersion::Http2, HttpVersion::Http1])?
            .call(op)
            .await
    }
}
```

The slice passed to `select_client()` would be the list from the Smithy protocol trait (`http` or `eventStreamHttp`
depending on th eoperation). The selection implementation will be an `O(n*m)` loop over the `ConnectorHttpVersion` list,
which will be fine since both `n` and `m` will only ever be 1-2 items, or if another protocol version is introduced
later, maybe 1-3.

If an operation requires a specific protocol version, and if the `connector_fn` can't provide that version,
then the `select_client()` function will return `SdkError::ConstructionFailure` indicating the error.

Changes Checklist
-----------------

- [ ] Create `HttpVersion` in `smithy-http` with `Http1_1` and `Http2`
- [ ] Refactor existing `https()` connector creation functions to take `HttpVersion`
- [ ] Add `connector_fn` to `SharedConfig`, and wire up the `https()` functions as a default
- [ ] Create a private `ConnectorHttpVersion` for use in the fluent clients
- [ ] Update `Handle` to have a `Vec<ConnectorHttpVersion>`
- [ ] Implement function to calculate a minimum required set of HTTP versions from a Smithy model in the code generator
- [ ] Update `new()` on fluent clients to construct the minimum set of required connectors to go into the `Handle`
- [ ] Implement `select_client` on `Handle`
- [ ] Update the fluent client `send()` function code gen to call `select_client()` with the preferred HTTP versions
- [ ] Add required defaulting for models that don't set the optional `http` and `eventStreamHttp` protocol trait attributes
