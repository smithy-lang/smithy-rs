# Upgrading from http@0.2/hyper@0.14 to http@1.x/hyper@1.x

This guide provides a comprehensive walkthrough for upgrading your smithy-rs server applications from http@0.2.x and hyper@0.14 to http@1.x and hyper@1.x.

## Table of Contents

- [Overview](#overview)
- [Why Upgrade?](#why-upgrade)
- [Before You Begin](#before-you-begin)
- [Dependency Updates](#dependency-updates)
- [Server-Side Changes](#server-side-changes)
- [Client-Side Changes](#client-side-changes)
- [Test Infrastructure Updates](#test-infrastructure-updates)
- [Common Migration Patterns](#common-migration-patterns)
- [Troubleshooting](#troubleshooting)
- [Migration Checklist](#migration-checklist)

## Overview

The http and hyper crates have released major version updates (http@1.x and hyper@1.x) with significant API improvements and breaking changes. This guide helps you migrate your smithy-rs server applications to these new versions.

**Key Changes:**
- http: 0.2.x → 1.x
- hyper: 0.14.x → 1.x
- New hyper-util crate for additional utilities

## Why Upgrade?

- **Improved API**: More ergonomic and safer APIs in both http and hyper
- **Active Support**: Future updates and bug fixes will target 1.x versions
- **Ecosystem Alignment**: New libraries are targeting http@1.x and hyper@1.x
- **Security Updates**: Continued security patches for 1.x line

## Before You Begin

**Important Considerations:**

1. **Breaking Changes**: This is a major version upgrade with breaking API changes
2. **Testing Required**: Thoroughly test your application after migration
3. **Gradual Migration**: Consider migrating one service at a time
4. **Legacy Examples**: The `examples/legacy/` directory contains fully working http@0.2 examples for reference

## Dependency Updates

### Cargo.toml Changes

#### Server Dependencies

**Before (http@0.2/hyper@0.14):**
```toml
[dependencies]
http = "0.2"
hyper = { version = "0.14.26", features = ["server"] }
tokio = "1.26.0"
tower = "0.4"
```

**After (http@1.x/hyper@1.x):**
```toml
[dependencies]
http = "1"
hyper = { version = "1", features = ["server"] }
hyper-util = { version = "0.1", features = ["tokio", "server", "server-auto", "service"] }
tokio = { version = "1.26.0", features = ["rt-multi-thread", "macros"] }
tower = "0.4"
```

**Key Changes:**
- `http`: `0.2` → `1`
- `hyper`: `0.14.26` → `1`
- **New**: `hyper-util` crate for server and client utilities
- **New**: `bytes` and `http-body-util` for body handling
- `hyper-rustls`: `0.24` → `0.27`

## Server-Side Changes

### 1. Server Initialization

**Before (hyper@0.14):**
```rust
#[tokio::main]
pub async fn main() {
    // ... setup config and app ...

    let make_app = app.into_make_service_with_connect_info::<SocketAddr>();

    let bind: SocketAddr = format!("{}:{}", args.address, args.port)
        .parse()
        .expect("unable to parse the server bind address and port");

    let server = hyper::Server::bind(&bind).serve(make_app);

    if let Err(err) = server.await {
        eprintln!("server error: {err}");
    }
}
```

**After (hyper@1.x):**
```rust
#[tokio::main]
pub async fn main() {
    // ... setup config and app ...

    let make_app = app.into_make_service_with_connect_info::<SocketAddr>();

    let bind: SocketAddr = format!("{}:{}", args.address, args.port)
        .parse()
        .expect("unable to parse the server bind address and port");

    let listener = TcpListener::bind(bind)
        .await
        .expect("failed to bind TCP listener");

    // Optional: Get the actual bound address (useful for port 0)
    let actual_addr = listener.local_addr().expect("failed to get local address");
    eprintln!("Server listening on {}", actual_addr);

    if let Err(err) = serve(listener, make_app).await {
        eprintln!("server error: {err}");
    }
}
```

**Key Changes:**
1. Replace `hyper::Server::bind(&bind)` with `TcpListener::bind(bind).await`
2. Use the `serve()` helper function instead of `.serve(make_app)`
3. Can get actual bound address with `.local_addr()` (useful for testing with port 0)

### 2. Service Building

The service building API remains the same:

```rust
let app = PokemonService::builder(config)
    .get_pokemon_species(get_pokemon_species)
    .get_storage(get_storage_with_local_approved)
    .get_server_statistics(get_server_statistics)
    .capture_pokemon(capture_pokemon)
    .do_nothing(do_nothing_but_log_request_ids)
    .check_health(check_health)
    .build()
    .expect("failed to build an instance of PokemonService");

let make_app = app.into_make_service_with_connect_info::<SocketAddr>();
```

No changes needed here! The service builder API is stable.

## Client-Side Changes

### HTTP Client Connector Setup

**Before (hyper@0.14 with hyper-rustls):**
```rust
use aws_smithy_runtime::client::http::hyper_014::HyperClientBuilder;
use hyper_rustls::ConfigBuilderExt;

fn create_client() -> PokemonClient {
    let tls_config = rustls::ClientConfig::builder()
        .with_safe_defaults()
        .with_native_roots()
        .with_no_client_auth();

    let tls_connector = hyper_rustls::HttpsConnectorBuilder::new()
        .with_tls_config(tls_config)
        .https_or_http()
        .enable_http1()
        .enable_http2()
        .build();

    let http_client = HyperClientBuilder::new().build(tls_connector);

    let config = pokemon_service_client::Config::builder()
        .endpoint_url(POKEMON_SERVICE_URL)
        .http_client(http_client)
        .build();

    pokemon_service_client::Client::from_conf(config)
}
```

**After (http@1.x with aws-smithy-http-client):**
```rust
use aws_smithy_http_client::{Builder, tls};

fn create_client() -> PokemonClient {
    // Create a TLS context with platform trusted root certificates
    let tls_context = tls::TlsContext::builder()
        .with_trust_store(tls::TrustStore::default())
        .build()
        .expect("failed to build TLS context");

    // Create an HTTP client using rustls with AWS-LC crypto provider
    let http_client = Builder::new()
        .tls_provider(tls::Provider::Rustls(tls::rustls_provider::CryptoMode::AwsLc))
        .tls_context(tls_context)
        .build_https();

    let config = pokemon_service_client::Config::builder()
        .endpoint_url(POKEMON_SERVICE_URL)
        .http_client(http_client)
        .build();

    pokemon_service_client::Client::from_conf(config)
}
```

**Key Changes:**
1. Replace `HyperClientBuilder` with `aws_smithy_http_client::Builder`
2. Use `tls::TlsContext` and `tls::TrustStore` instead of direct rustls config
3. Specify TLS provider explicitly (Rustls with AWS-LC or Ring crypto)
4. Use `.build_https()` instead of passing a connector

### Using Default Client

If you don't need custom TLS configuration, you can use the default client:

```rust
// Default client with system trust store
let config = pokemon_service_client::Config::builder()
    .endpoint_url(POKEMON_SERVICE_URL)
    .build();

let client = pokemon_service_client::Client::from_conf(config);
```

## Common Migration Patterns

### 1. Response Building

**Before:**
```rust
use hyper::{Body, Response, StatusCode};

let response = Response::builder()
    .status(StatusCode::OK)
    .body(Body::from("Hello"))
    .unwrap();
```

**After:**
```rust
use aws_smithy_http_server::http::{Response, StatusCode};
use http_body_util::Full;
use bytes::Bytes;

let response = Response::builder()
    .status(StatusCode::OK)
    .body(Full::new(Bytes::from("Hello")))
    .unwrap();
```

### 2. Middleware and Layers

Tower layers continue to work the same way:

```rust
// No changes needed for tower layers
let config = PokemonServiceConfig::builder()
    .layer(AddExtensionLayer::new(Arc::new(State::default())))
    .layer(AlbHealthCheckLayer::from_handler("/ping", |_req| async {
        StatusCode::OK
    }))
    .layer(ServerRequestIdProviderLayer::new())
    .build();
```

## Troubleshooting

### Common Errors and Solutions

#### 1. "no method named `serve` found for struct `hyper::Server`"

**Error:**
```
error[E0599]: no method named `serve` found for struct `hyper::Server` in the current scope
```

**Solution:**
Use `TcpListener` with the `serve()` function instead:
```rust
let listener = TcpListener::bind(bind).await?;
serve(listener, make_app).await?;
```

#### 2. "the trait `tower::Service` is not implemented"

**Error:**
```
error[E0277]: the trait `tower::Service<Request<Incoming>>` is not implemented for `...`
```

**Solution:**
Make sure you're using `hyper-util` with the right features:
```toml
hyper-util = { version = "0.1", features = ["tokio", "server", "server-auto", "service"] }
```

#### 3. "cannot find `HyperClientBuilder` in module"

**Error:**
```
error[E0432]: unresolved import `aws_smithy_runtime::client::http::hyper_014::HyperClientBuilder`
```

**Solution:**
Use the new client builder:
```rust
use aws_smithy_http_client::Builder;

let http_client = Builder::new()
    .build_https();
```

#### 4. Type mismatch with `Body` or `Bytes`

**Error:**
```
error[E0308]: mismatched types
expected struct `hyper::body::Incoming`
found struct `http_body_util::combinators::boxed::UnsyncBoxBody<...>`
```

**Solution:**
Add `http-body-util` and `bytes` dependencies:
```toml
bytes = "1"
http-body-util = "0.1"
```

Then use the appropriate body types from `http-body-util`.

### Getting Help

- **Examples**: Check the `examples/` directory for working http@1.x code
- **Legacy Examples**: Check `examples/legacy/` for http@0.2 reference
- **Documentation**: https://docs.rs/aws-smithy-http-server/
- **GitHub Issues**: https://github.com/smithy-lang/smithy-rs/issues

