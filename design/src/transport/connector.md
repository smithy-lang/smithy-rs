The Smithy client provides a default TLS connector, but a custom one can be plugged in.
`rustls` is enabled with the feature flag `rustls`.

The client had previously supported `native-tls`. You can use your custom connector like this.

Create your connector:

```rust
/// A `hyper` connector that uses the `native-tls` crate for TLS. To use this in a smithy client,
/// wrap it in a [hyper_ext::Adapter](crate::hyper_ext::Adapter).
pub type NativeTls = hyper_tls::HttpsConnector<hyper::client::HttpConnector>;

pub fn native_tls() -> NativeTls {
    let mut tls = hyper_tls::native_tls::TlsConnector::builder();
    let tls = tls
        .min_protocol_version(Some(hyper_tls::native_tls::Protocol::Tlsv12))
        .build()
        .unwrap_or_else(|e| panic!("Error while creating TLS connector: {}", e));
    let mut http = hyper::client::HttpConnector::new();
    http.enforce_http(false);
    hyper_tls::HttpsConnector::from((http, tls.into()))
}
```

Plug the connector in the client:
```rust
let mut builder = hyper::client::Builder::default();
builder.pool_max_idle_per_host(70);
let connector = aws_smithy_client::erase::DynConnector::new(
    aws_smithy_client::hyper_ext::Adapter::builder()
        .hyper_builder(builder)
        .connector_settings(std::default::Default::default())
        .build(native_tls()),
);
let raw_client = aws_smithy_client::builder::Builder::new()
    .connector(connector)
    .middleware_fn(...)
    .build_dyn();
```
