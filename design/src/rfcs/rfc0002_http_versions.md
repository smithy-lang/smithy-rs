RFC: Supporting multiple HTTP versions for SDKs that use Event Stream
=====================================================================

> Status: Accepted

For a summarized list of proposed changes, see the [Changes Checklist](#changes-checklist) section.


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
- **Smithy Client**: A `aws_smithy_client::Client<C, M, R>` struct that is responsible for gluing together
  the connector, middleware, and retry policy. This isn't intended to be used directly.
- **Fluent Client**: A code generated `Client<C, M, R>` that has methods for each service operation on it.
  A fluent builder is generated alongside it to make construction easier.
- **AWS Client**: A specialized Fluent Client that uses a `DynConnector`, `DefaultMiddleware`,
  and `Standard` retry policy.

All of these are just called `Client` in code today. This is something that could be clarified in a separate refactor.

How Clients Work Today
----------------------

Fluent clients currently keep a handle to a single Smithy client, which is a wrapper
around the underlying connector. When constructing operation builders, this handle is `Arc` cloned and
given to the new builder instances so that their `send()` calls can initiate a request.

The generated fluent client code ends up looking like this:
```rust,ignore
struct Handle<C, M, R> {
    client: aws_smithy_client::Client<C, M, R>,
    conf: crate::Config,
}

pub struct Client<C, M, R = Standard> {
    handle: Arc<Handle<C, M, R>>,
}
```

Functions are generated per operation on the fluent client to gain access to the individual operation builders.
For example:

```rust,ignore
pub fn assume_role(&self) -> fluent_builders::AssumeRole<C, M, R> {
    fluent_builders::AssumeRole::new(self.handle.clone())
}
```

The fluent operation builders ultimately implement `send()`, which chooses the one and only Smithy client out
of the handle to make the request with:

```rust,ignore
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

```rust,ignore
let connector = Builder::new()
    .https()
    .middleware(...)
    .build();
let client = Client::with_config(connector, Config::builder().build());
```

The `https()` method on the Builder constructs the actual Hyper client, and is driven off Cargo features to
select the correct TLS implementation. For example:

```rust
#[cfg(feature = "__rustls")]
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

To accomplish this, `SharedConfig` will have a `make_connector` member. A customer would configure
it as such:

```rust,ignore
let config = some_shared_config_loader()
    .with_http_settings(my_http_settings)
    .with_make_connector(|reqs: &MakeConnectorRequirements| {
        Some(MyCustomConnector::new(reqs))
    })
    .await;
```

The passed in `MakeConnectorRequirements` will hold the customer-provided `HttpSettings` as well
as any Smithy-modeled requirements, which will just be `HttpVersion` for now. The `MakeConnectorRequirements`
struct will be marked `non_exhaustive` so that new requirements can be added to it as the SDK evolves.

A default `make_connector` implementation would be provided that creates a Hyper connector based on the
Cargo feature flags. This might look something like this:

```rust
#[cfg(feature = "__rustls")]
pub fn default_connector(reqs: &HttpRequirements) -> HyperAdapter {
    let https = hyper_rustls::HttpsConnector::with_native_roots();
    let mut builder = hyper::Client::builder();
    builder = configure_settings(builder, &reqs.http_settings);
    if let Http2 = &reqs.http_version {
        builder = builder.http2_only(true);
    }
    HyperAdapter::from(builder.build::<_, SdkBody>(https))
}
```

For any given service, `make_connector` could be called multiple times to create connectors
for all required HTTP versions and settings.

**Note:** the `make_connector` returns an `Option` since an HTTP version may not be required, but rather, preferred
according to a Smithy model. For operations that list out `["h2", "HTTP/1.1"]` as the desired versions,
a customer could choose to provide only an HTTP 1 connector, and the operation should still succeed.

Solving the Connector Selection Problem
---------------------------------------

Each service operation needs to be able to select a connector that meets its requirements best
from the customer provided connectors. Initially, the only selection criteria will be the HTTP version,
but later when per-operation HTTP settings are implemented, the connector will also need to be keyed off of those
settings. Since connector creation is not a cheap process, connectors will need to be cached after they are
created.

This caching is currently handled by the `Handle` in the fluent client, which holds on to the
Smithy client. This cache needs to be adjusted to:
- Support multiple connectors, keyed off of the customer provided `HttpSettings`, and also off of the Smithy modeled requirements.
- Be lazy initialized. Services that have a mix of Event Stream and non-streaming operations shouldn't create
  an HTTP/2 client if the customer doesn't intend to use the Event Stream operations that require it.

To accomplish this, the `Handle` will hold a cache that is optimized for many reads and few writes:

```rust,ignore
#[derive(Debug, Hash, Eq, PartialEq)]
struct ConnectorKey {
    http_settings: HttpSettings,
    http_version: HttpVersion,
}

struct Handle<C, M, R> {
    clients: RwLock<HashMap<HttpRequirements<'static>, aws_smithy_client::Client<C, M, R>>>,
    conf: crate::Config,
}

pub struct Client<C, M, R = Standard> {
    handle: Arc<Handle<C, M, R>>,
}
```

With how the generics are organized, the connector type will have to be the same between HTTP implementations,
but this should be fine since it is generally a thin wrapper around a separate HTTP implementor.
For cases where it is not, the custom connector type can host its own `dyn Trait` solution.

The `HttpRequirements` struct will hold `HttpSettings` as copy-on-write so that it can be used
for cache lookup without having to clone `HttpSettings`:

```rust,ignore
struct HttpRequirements<'a> {
    http_settings: Cow<'a, HttpSettings>,
    http_version: HttpVersion,
}

impl<'a> HttpRequirements<'a> {
    // Needed for converting a borrowed HttpRequirements into an owned cache key for cache population
    pub fn into_owned(self) -> HttpRequirements<'static> {
        Self {
            http_settings: Cow::Owned(self.http_settings.into_owned()),
            http_version: self.http_version,
        }
    }
}
```

With the cache established, each operation needs to be aware of its requirements. The code generator will be
updated to store a prioritized list of `HttpVersion` in the property bag in an input's `make_operation()` method.
This prioritized list will come from the Smithy protocol trait's `http` or `eventStreamHttp` attribute, depending
on the operation. The fluent client will then pull this list out of the property bag so that it can determine which
connector to use. This indirection is necessary so that an operation still holds all information
needed to make a service call from the Smithy client directly.

**Note:** This may be extended in the future to be more than just `HttpVersion`, for example, when per-operation
HTTP setting overrides are implemented. This doc is not attempting to solve that problem.

In the fluent client, this will look as follows:

```rust,ignore
impl<C, M, R> AssumeRole<C, M, R> where ... {
    pub async fn send(self) -> Result<AssumeRoleOutput, SdkError<AssumeRoleError>> where ... {
        let input = self.create_input()?;
        let op = input.make_operation(&self.handle.conf)?;

        // Grab the `make_connector` implementation
        let make_connector = self.config.make_connector();

        // Acquire the prioritized HttpVersion list
        let http_versions = op.properties().get::<HttpVersionList>();

        // Make the actual request (using default HttpSettings until modifying those is implemented)
        let client = self.handle
            .get_or_create_client(make_connector, &default_http_settings(), &http_versions)
            .await?;
        client.call(op).await
    }
}
```

If an operation requires a specific protocol version, and if the `make_connection` implementation can't
provide that it, then the `get_or_create_client()` function will return `SdkError::ConstructionFailure`
indicating the error.

Changes Checklist
-----------------

- [ ] Create `HttpVersion` in `aws-smithy-http` with `Http1_1` and `Http2`
- [ ] Refactor existing `https()` connector creation functions to take `HttpVersion`
- [ ] Add `make_connector` to `SharedConfig`, and wire up the `https()` functions as a default
- [ ] Create `HttpRequirements` in `aws-smithy-http`
- [ ] Implement the connector cache on `Handle`
- [ ] Implement function to calculate a minimum required set of HTTP versions from a Smithy model in the code generator
- [ ] Update the `make_operation` code gen to put an `HttpVersionList` into the operation property bag
- [ ] Update the fluent client `send()` function code gen grab the HTTP version list and acquire the correct connector with it
- [ ] Add required defaulting for models that don't set the optional `http` and `eventStreamHttp` protocol trait attributes
