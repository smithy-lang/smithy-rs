# SmithyRs Client Examples

This package contains some examples on how to use the Smithy Client to communicate
with a Smithy-based service.

## Pre-requisites

1. Build the `pokemon-service-client` and `pokemon-service` by invoking `make` in the
   [examples](https://github.com/awslabs/smithy-rs/tree/main/examples) folder.

```bash
make
```

2. Run the Pokemon service locally by issuing the following command from the
   [examples](https://github.com/awslabs/smithy-rs/tree/main/examples) folder. This
   will launch the Smithy-Rs based service on TCP port 13734.

```bash
cargo run --bin pokemon-service
```

## Running the examples

You can view a list of examples by running `cargo run --example` from the
[pokemon-service-client-usage](https://github.com/awslabs/smithy-rs/tree/main/examples/pokemon-service-client-usage)
folder. To run an example, pass its name to the `cargo run --example` command, e.g.:

```
cargo run --example simple-client
```

## List of examples

| Rust Example                  | Description                                                                                               |
| ----------------------------- | --------------------------------------------------------------------------------------------------------- |
| simple-client                 | Creates a Smithy Client and calls an operation on it.                                         |
| endpoint-resolver             | How to set a custom endpoint resolver                                   |
| endpoint-using-interceptor    | A custom middleware layer for setting endpoint on a request.                                              |
| handling-errors               | How to send an input parameter to an operation, and to handle errors.                                     |
| custom-header                 | How to add headers to a request.                                        |
| custom-header-using-interceptor | How to access operation name being called in an interceptor. |
| requestid-interceptor   | Modify the request before it is serialized and add a request ID to it. |
| response-header-interceptor   | How to get operation name and access response before it is deserialized |
| use-config-bag            | How to use the property bag to pass data across interceptors.             |
| retries-customize             | Customize retry settings. |
| retries-disable               | How to disable retries.                                                 |
| timeout-config                | How to configure timeouts.                  |
| mock-request                  | Use a custom HttpConnector / Client to generate mock responses. |
| interceptor-errors             | Raising an error from an interceptor and detecting it at the origin of the call. |
| trace-serialize             | Trace request and response as they are being sent and received on the wire. |
| client-connector            | Shows how to change TLS related configuration |
