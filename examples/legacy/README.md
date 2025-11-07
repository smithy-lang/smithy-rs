# Legacy HTTP 0.x Examples

This directory contains examples for Smithy-rs using HTTP 0.x (hyper 0.14, http 0.2). These examples use the legacy HTTP stack with `aws-smithy-legacy-http` and `aws-smithy-legacy-http-server`.

For HTTP 1.x examples (hyper 1.x, http 1.x), see the parent [examples](../) directory.

## Building

### 1. Generate the SDKs

From this directory, run:

```bash
make codegen
```

This will generate:
- `pokemon-service-server-sdk-http0x` - Server SDK using HTTP 0.x
- `pokemon-service-client-http0x` - Client SDK using HTTP 0.x

The generated SDKs are copied to:
- `pokemon-service-server-sdk/`
- `pokemon-service-client/`

### 2. Build all examples

```bash
cargo build
```

Or to check without building artifacts:

```bash
cargo check
```

## Running the Examples

### Start the Pokemon Service

In one terminal, start the server:

```bash
cargo run --bin pokemon-service
```

The server will start on `http://localhost:13734`

### Run Client Examples

In another terminal, from the `pokemon-service-client-usage/` directory:

```bash
cd pokemon-service-client-usage
cargo run --example simple-client
```

#### Available Client Examples

| Example | Description |
|---------|-------------|
| `simple-client` | Basic client usage - creates a client and calls an operation |
| `endpoint-resolver` | Custom endpoint resolver configuration |
| `handling-errors` | Sending input parameters and handling errors |
| `custom-header` | Adding custom headers to requests |
| `custom-header-using-interceptor` | Accessing operation name in an interceptor |
| `response-header-interceptor` | Getting operation name and accessing response before deserialization |
| `use-config-bag` | Using the property bag to pass data across interceptors |
| `retry-customize` | Customizing retry settings |
| `timeout-config` | Configuring timeouts |
| `mock-request` | Using custom HttpConnector for mock responses |
| `trace-serialize` | Tracing request/response during serialization |
| `client-connector` | Changing TLS configuration |

To list all available examples:

```bash
cd pokemon-service-client-usage
cargo run --example
```

### Other Services

#### Pokemon Service with TLS

```bash
cargo run --bin pokemon-service-tls
```

#### Pokemon Service on AWS Lambda

```bash
cargo run --bin pokemon-service-lambda
```

## Project Structure

```
legacy/
├── pokemon-service/              # Main HTTP service implementation
├── pokemon-service-tls/          # TLS-enabled service
├── pokemon-service-lambda/       # AWS Lambda service
├── pokemon-service-common/       # Shared service logic
├── pokemon-service-client-usage/ # Client usage examples
├── pokemon-service-server-sdk/   # Generated server SDK (HTTP 0.x)
└── pokemon-service-client/       # Generated client SDK (HTTP 0.x)
```

## Key Dependencies (HTTP 0.x)

- `hyper = "0.14"`
- `http = "0.2"`
- `aws-smithy-legacy-http`
- `aws-smithy-legacy-http-server`

## Regenerating SDKs

If you need to regenerate the SDKs from scratch:

```bash
rm -rf pokemon-service-server-sdk pokemon-service-client
make codegen
```

## Testing

Run all tests:

```bash
cargo test
```

Run tests for a specific package:

```bash
cargo test -p pokemon-service
```

## Troubleshooting

### Port Already in Use

If port 13734 is already in use, you can specify a different port:

```bash
cargo run --bin pokemon-service -- --port 8080
```

Then update the client examples to use the new port by setting the environment variable:

```bash
POKEMON_SERVICE_URL=http://localhost:8080 cargo run --example simple-client
```

### SDK Generation Issues

If the generated SDKs have issues, try cleaning and regenerating:

```bash
# Clean generated SDKs
rm -rf pokemon-service-server-sdk pokemon-service-client

# Clean gradle cache
cd ../..
./gradlew clean

# Regenerate
cd examples/legacy
make codegen
```

## Migration to HTTP 1.x

For new projects, we recommend using the HTTP 1.x examples in the parent [examples](../) directory. These legacy examples are maintained for backward compatibility and for projects that need to use the HTTP 0.x stack.

The main differences:
- HTTP 1.x uses `hyper 1.x`, `http 1.x`
- HTTP 1.x uses `aws-smithy-http`, `aws-smithy-http-server` (not legacy versions)
- HTTP 1.x has better performance and modern async runtime support
