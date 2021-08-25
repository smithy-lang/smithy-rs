RFC: Supporting multiple HTTP versions for SDKs that use Event Stream
=====================================================================

> Status: RFC. For a summarized list of proposed changes, see the [Changes Checklist](#changes-checklist) section.

Most AWS SDK operations use HTTP/1.1, but bi-directional streaming operations that use the Event Stream
message framing format need to use HTTP/2 (h2).

Smithy models can also customize which HTTP versions are used in each individual protocol trait.
For example,
[`@restJson1` has attributes `http` and `eventStreamHttp`](https://awslabs.github.io/smithy/1.0/spec/aws/aws-restjson1-protocol.html#aws-protocols-restjson1-trait)
to list out the versions that should be used in a priority order.

This document proposes adding a mechanism for the generated operation fluent builders to be able to
select the correct HTTP version.

Current Client Structure
------------------------

Generated service clients currently keep a handle to a single `smithy-client::Client`, which is a wrapper
around the underlying connector (typically a Hyper client). When constructing operation builders, this handle
is `Arc` cloned and given to the new builder instances so that their `send()` calls can initiate a request.

The generated client code ends up looking like this:
```rust
struct Handle<C, M, R> {
    client: smithy_client::Client<C, M, R>,
    conf: crate::Config,
}

pub struct Client<C, M, R = Standard> {
    handle: Arc<Handle<C, M, R>>,
}
```

Functions are generated per operation on this `Client` to gain access to the individual operation builders.
For example:

```rust
pub fn assume_role(&self) -> fluent_builders::AssumeRole<C, M, R> {
    fluent_builders::AssumeRole::new(self.handle.clone())
}
```

The fluent operation builders ultimately implement `send()`, which chooses the one and only `client` out
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

Smithy generic clients are constructed from an underlying client, as shown:

```rust
let underlying_client = Builder::new()
    .https()
    .middleware(...)
    .build();
let client = Client::with_config(underlying_client, Config::builder().build());
```

The `https()` method on the Builder constructs the actual Hyper client, and is driven off Cargo features to
select the correct TLS implementation. For example:

```rust
#[cfg(feature = "rustls")]
pub fn https() -> Https {
    let https = hyper_rustls::HttpsConnector::with_native_roots();
    let client = hyper::Client::builder().build::<_, SdkBody>(https);
    crate::hyper_impls::HyperAdapter::from(client)
}
```

For the AWS SDKs, the client construction is behind the scenes and any changes there can be made without
changing the external API.

Proposed Client Structure
-------------------------

To enable version selection, the `Handle` should hold a list of clients wrapped in a `ClientProtocol` enum:

```rust
enum ClientProtocol {
    Http1_1(smithy_client::Client<C, M, R>),
    Http2(smithy_client::Client<C, M, R>),
}

struct Handle<C, M, R> {
    clients: Vec<ClientProtocol>,
    conf: crate::Config,
}

pub struct Client<C, M, R = Standard> {
    handle: Arc<Handle<C, M, R>>,
}
```

This makes the required HTTP versions available to `send()`. In order to construct a client now,
multiple clients may need to be specified. A builder will be provided to make this easy:

```rust
let http1_client = Builder::new()
    .https1_1()
    .middleware(...)
    .build();

let h2_client = Builder::new()
    .https2()
    .middleware(...)
    .build();

let client = Client::with_config(
    ClientProtocol::builder()
        .http1_1(http1_client)
        .http2(h2_client)
        .build(),
    Config::builder().build()
);
```

The code generator knows which HTTP version is preferred for any given operation, so when it generates
the operation `send()` method, it can list out the prioritized list of `ClientProtocol`:

```rust
impl<C, M, R> AssumeRole<C, M, R> where ...{
    pub async fn send(self) -> Result<AssumeRoleOutput, SdkError<AssumeRoleError>> where ... {
        // Setup code omitted ...

        // Make the actual request
        self.handle.prefer_client(ClientProtocolSelection::Http2)
            .or_else(ClientProtocolSelection::Http1_1)
            .acquire()?
            .call(op)
            .await
    }
}
```

The `prefer_client/or_else` chain would be the list from the Smithy protocol trait (`http` or `eventStreamHttp`
depending on the operation). The selection implementation will be an `O(n*m)` loop over the `ClientProtocol` list,
which will be fine since both `n` and `m` will only ever be 1-2 items, or if another protocol version is introduced
later, maybe 1-3.

If an operation requires a specific protocol version, and if the service client is constructed without that version,
then the `acquire()` function will return `SdkError::ConstructionFailure` indicating the mistake.

Changes Checklist
-----------------

- [ ] Create `ClientProtocol` in `smithy-http` with `Http1_1` and `Http2`
- [ ] Create `ClientProtocol::builder()` implementation
- [ ] Create `ClientProtocolSelection` in `smithy-http` that is a mirror of `ClientProtocol` without the client member
- [ ] Implement reusable `select_client` function in `smithy-http` that will be used by the `send()` methods
- [ ] Update the AWS client construction code gen to construct the different `ClientProtocols` that are used for the service being generated
- [ ] Update the `send()` code gen to pull preferred versions from the Smithy protocol traits
- [ ] Add required defaulting for models that don't set the optional `http` and `eventStreamHttp` protocol trait attributes
