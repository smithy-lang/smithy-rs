RFC: User-friendly Configurable Connectors
==========================================

> Status: RFC

*This RFC builds upon [RFC-0002: Supporting multiple HTTP versions for SDKs that use Event Stream](./rfc0002_http_versions.md), make sure you're familiar with it.*

At the core of the SDKs is our networking stack: that which allows a user to make a request to an API and receive a response. The stack has many layers and makes multiple HTTP requests for every API request. This RFC defines how the **Connector**s necessary for making HTTP requests should be created, configured, and cached for re-use.

Terminology
-----------

- **SDK Config**: A struct for storing configuration that's needed by all **AWS SDK Crate**s. Defined in the [`aws-types`](https://docs.rs/aws-types/latest/aws_types/) **AWS Runtime Crate**, with related functionality defined in the [`aws-config`](https://docs.rs/aws-config/latest/aws_config/) **AWS Runtime Crate**.
- **Service Config**: A struct for storing configuration that's needed for a specific **AWS SDK Crate**, like [`aws-sdk-s3::Config`](https://docs.rs/aws-sdk-s3/latest/aws_sdk_s3/struct.Config.html).
- **Service Client**: A struct for storing a **Service Config** and a **Smithy Client Pool**, like [`aws-sdk-s3::Client`](https://docs.rs/aws-sdk-s3/latest/aws_sdk_s3/struct.Client.html). **Service Client**s are created from **Service Config**s and have convenience methods for create **Fluent Builder**s which can then be used to make API requests.
- **Fluent Builder**: A builder struct for constructing a specific API request that can then be sent. For example, [`aws_sdk_s3::client::fluent_builders::ListBuckets`](https://docs.rs/aws-sdk-s3/latest/aws_sdk_s3/client/fluent_builders/struct.ListBuckets.html).
- **Smithy Client Pool**: A **shared** struct that maintains a pool of [Smithy Clients](https://docs.rs/aws-smithy-client/latest/aws_smithy_client/struct.Client.html). Analogous to a [connection pool](https://en.wikipedia.org/wiki/Connection_pool).
- **AWS SDK Crate**: A crate that provides a client for calling a given AWS service, such as [`aws-sdk-s3`](https://docs.rs/aws-sdk-s3/latest/aws_sdk_s3/) for calling S3.
- **AWS Runtime Crate**: Any runtime crate that the AWS SDK generated code relies on, such as [`aws-types`](https://docs.rs/aws-types/latest/aws_types/).
- **Smithy Runtime Crate**: Any runtime crate that the smithy-rs generated code relies on, such as [`aws-smithy-types`](https://docs.rs/aws-smithy-types/latest/aws_smithy_types/).
- **Connector**: An implementor of Tower's `Service` trait that converts a request into a response. This is typically
  a thin wrapper around a [`hyper` client](https://docs.rs/hyper/latest/hyper/client/index.html).
- **Shared**: By convention, a prefix for structs that are wrapped in an [`Arc`](https://doc.rust-lang.org/stable/std/sync/struct.Arc.html) so that they can be cloned and passed around easily.

The proposed process of making a "list buckets" request to S3, at a glance
--------------------------------------------------------------------------

The code for making a request can look quite simple, but it hides a good amount of complexity. In order to understand whats really happening, let's look at an example:

```rust
#[tokio::main]
async fn main() {
  let sdk_config = aws_config::from_env().load().await;
  let client = aws_sdk_s3::Client::new(&sdk_config);
  let resp = client.list_buckets().send().await?;
  let bucket_list = resp.buckets().unwrap();

  println!("{:?}", bucket_list);
}
```

Now let's examine what happens on each line.

### `let sdk_config = aws_config::load_from_env().await;`

Environment variables and AWS profiles are read in order to build an `SdkConfig`. During Config construction, HTTP requests may be made in order to resolve user credentials. In order to do this, the following must be configured:

- Timeouts
- Retry limits and strategies
- An [`HttpConnector`](https://github.com/awslabs/smithy-rs/blob/107194f106a736426caccbd158e9fb756584ceb0/rust-runtime/aws-smithy-client/src/http_connector.rs#L20) containing either a prebuilt [`DynConnector`](https://github.com/awslabs/smithy-rs/blob/107194f106a736426caccbd158e9fb756584ceb0/rust-runtime/aws-smithy-client/src/erase.rs#L143) or a function for creating one (a [`MakeConnectorFn`](https://github.com/awslabs/smithy-rs/blob/107194f106a736426caccbd158e9fb756584ceb0/rust-runtime/aws-smithy-client/src/http_connector.rs#L15))
- An async sleep implementation (this can be omitted if timeouts and retries are disabled)

If any of these settings are unset, defaults may be provided unless [default features](https://doc.rust-lang.org/cargo/reference/features.html#the-default-feature) have been disabled.

*NOTE: It's possible to explicitly disable timeouts and retries.*

Users may customize the config by calling various methods on the `ConfigLoader` returned by `aws_config::from_env`. Once `load()` is called, the resulting `SdkConfig` is immutable and can't be modified, only replaced.

### `let client = aws_sdk_s3::Client::new(&sdk_config);`

At this stage, the general-use `SdkConfig` is transformed into a special-use `aws_sdk_s3::Config` which, in turn, is used to create an `aws_sdk_s3::Client`.

#### The Service Config

For a majority of services, the service config doesn't contain anything more than what was set in the `SdkConfig`, with a notable exception. The service config is responsible for providing a service-specific middleware called the "Default Middleware" that:

- Uses SigV4 to sign requests
- Attaches credentials to requests
- Attaces a user agent to requests
- Attaches info that's used for recursion detection
- Resolves service endpoints

When calling `aws_sdk_s3::Client::new`, the `SdkConfig` is transformed into an `aws_sdk_s3::Config` by calling an `Into::into` impl.

The service config could also afford users the ability to wrap this middleware or replace it, but that's outside the scope of this RFC.

#### The Service Client

The Service Client wraps the Service Config and the Smithy Client Pool. When used to create a Fluent Builder, it passes on copies of both. In previous versions of smithy-rs, the Service Client was also responsible for creating a Smithy Client, but that responsibility is now held by a Fluent Builder's `send()` method.

### `let resp = client.list_buckets().send().await?;`

Calling `client.list_buckets()` creates a Fluent Builder for the ["list buckets" operation](https://docs.rs/aws-sdk-s3/latest/aws_sdk_s3/operation/struct.ListBuckets.html). The magic happens when `send().await` is called, and that's where knowledge of [RFC-0002](./rfc0002_http_versions.md) is necessary.

1. The Fluent Builder constructs the inner Operation, passing it a copy of the Service Config and the Smithy Client Pool.
2. All operations have, in the HTTP request Property Bag, a priority list of HTTP versions to use when making requests. That list is fetched from the property bag.
3. A `MakeConnectorSettings` is created by calling an `Into::into` impl on the Service Config.
4. Using those settings, and the priority list of HTTP versions, a client is fetched from the Smithy Client Pool.
5. Finally, the request is sent by calling `client.call(operation).await`

### `let bucket_list = resp.buckets().unwrap();`

There's nothing new to be seen here. The previous line sent the request and recieved the response. This line is just looking at the response.

The inner workings of the Smithy Client Pool
--------------------------------------------

While the Service Client "owns" the Smithy Client Pool, its only responsibility is to share copies of it with Operations. The client pool has three methods:

- `pub async fn get_or_create_client<E>(&self, make_connector_settings: MakeConnectorSettings, http_versions: &[http::Version]) -> Result<Arc<aws_smithy_client::Client>, SdkError<E>>`
- `async fn fetch_existing_client(&self, connector_key: &ConnectorKey) -> Option<Arc<aws_smithy_client::Client>>`
- `async fn initialize_and_store_new_client<E>(&self, connector_key: &ConnectorKey) -> Result<Arc<aws_smithy_client::Client>, SdkError<E>>`

Let's look at each in turn, starting with the sole public method.

### `get_or_create_client<E>`

Smithy Clients are created lazily. If a user never makes a request, than no clients get created. If they only make `HTTP/1.1` requests, then only an `HTTP/1.1` is ever created. When users call `send()` on an Operation, that in turn calls this method. Here's what the implementation could look like:

```rust
pub async fn get_or_create_client<E>(
    &self,
    make_connector_settings: &MakeConnectorSettings,
    http_versions: &[http::Version],
) -> Result<
    Arc<aws_smithy_client::Client>,
    SdkError<E>
> {
    // http_versions should NEVER be empty, but it can't hurt to check
    if http_versions.is_empty() {
        return Err(SdkError::ConstructionFailure(Box::new(
            HttpConnectorError::NoHttpVersionsSpecified,
        )));
    }

    // We could make this a Vec if we wanted to return all errors
    // in the case that we can't successfully create ANY clients
    let mut construction_failure = None;

    // Iterater over the priority list of HTTP versions
    for http_version in http_versions {
        // Create the key used to fetch an existing client if one exists
        let connector_key = ConnectorKey {
            make_connector_settings: make_connector_settings.clone(),
            http_version: http_version.clone(),
        };

        // Try to fetch an existing client and return early if we find one
        if let Some(client) = self.fetch_existing_client(&connector_key).await {
            return Ok(client);
        }

        // Otherwise, create the new client, store a copy of it in the client cache, and then return it
        match self
            .initialize_and_store_new_client(connector_key)
            .await
        {
            Err(err) => { construction_failure = Some(err); }
            client => return client,
        }
    }

    // If we haven't successfully returned already, then we'll surface a construction error here.
    // We only surface the last construction error from the loop above.
    Err(construction_failure.expect(
        "We early return on success so this must contain an error and is therefore safe to unwrap"
    ))
}
```

This function tries to fetch or create a client for each requested HTTP version, returning the first one it can get its grubby little hands on.
In the case that no clients are to be found in the Smithy Client Pool, and all attempts at client creation fail, then it will return the error
produced by the most recent failed client creation attempt.

### `fetch_existing_client`

```rust
async fn fetch_existing_client(
    &self,
    connector_key: &ConnectorKey
) -> Option<std::sync::Arc<aws_smithy_client::Client>> {
    let clients = self.clients.read().await;
    clients.get(connector_key).cloned()
}
```

There's not much to be said about this method. We check for a client in the pool, returning `Some(client)` if there is one and `None` if there isn't.

### `initialize_and_store_new_client<E>`

```rust

async fn initialize_and_store_new_client<E>(
    &self,
    connector_key: ConnectorKey,
) -> Result<
    Arc<aws_smithy_client::Client>,
    SdkError<E>
> {
    let sleep_impl = self.conf.sleep_impl;

    // Grab the HTTP Connector stored in the Service Config
    let connector = match &self.conf.http_connector {
        Some(connector) => Ok(connector.clone()),
        // If no HttpConnector was specified by the user, we grab the default one
        // This will fail if default features have been disabled
        None => HttpConnector::try_default()
            .map_err(|err| SdkError::ConstructionFailure(err.into())),
    }?
    .load(&connector_key.make_connector_settings, sleep_impl.clone())
    .map_err(|err| SdkError::ConstructionFailure(err.into()))?;

    let middleware = DynMiddleware::new(self.conf.default_middleware());

    // The rest of this is taken from how we create smithy clients in the SDK today
    let mut builder = aws_smithy_client::Builder::new()
        .connector(connector)
        .middleware(middleware);

    let retry_config = self.conf.retry_config.as_ref().cloned().unwrap_or_default();
    let timeout_config = self.conf.timeout_config.as_ref().cloned().unwrap_or_default();
    builder.set_retry_config(retry_config.into());
    builder.set_timeout_config(timeout_config);

    // the builder maintains a `TriState`. To avoid suppressing the warning when sleep is unset,
    // only set it if we actually have a sleep impl.
    if let TriState::Set(sleep_impl) = sleep_impl {
        builder.set_sleep_impl(Some(sleep_impl.clone()));
    }

    let client = Arc::new(builder.build());
    let mut clients = self.clients.write().await;
    clients.insert(connector_key, client.clone());

    Ok(client)
}
```

This method is very similar to the existing [`<service>::Client::from_conf_conn`](https://github.com/awslabs/smithy-rs/blob/e6d09ea686c1be31b7df0c24d87bc370173fb7b0/aws/sdk-codegen/src/main/kotlin/software/amazon/smithy/rustsdk/AwsFluentClientDecorator.kt#L132) function except that it gets a connector from the Service Config instead of as an argument.

Changes Checklist
-----------------

- [ ] Make a service's default middleware accessible from its corresponding config
- [x] Update codegen to store an operation's preferred HTTP versions in the property bag
- [x] Break up timeout config into separate pieces (api, http, tcp) so that we don't have to pass all of it through to layers of the networking stack where it doesn't belong.
- [ ] Update tests that rely on HTTP connectors to create those connectors with `MakeConnectorFn`s
- [ ] Implement proposed changes to service codegen
