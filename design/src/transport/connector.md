The Smithy client provides a default TLS connector, but a custom one can be
plugged in. `rustls` is enabled with the feature flag `rustls`.

The client had previously (prior to version 0.56.0) supported `native-tls`. You
can continue to use a client whose TLS implementation is backed by `native-tls`
by passing in a custom connector. Check out the `custom_connectors.rs` tests in
the `pokemon-service-tls` example crate.
