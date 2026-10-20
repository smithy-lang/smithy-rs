# Runtime protocol selection

One generated Rust client can use REST JSON, REST XML, AWS JSON 1.0, AWS JSON 1.1,
or RPCv2 CBOR without regenerating the SDK. This example starts a server on an
ephemeral loopback port and calls its greeting operation with each protocol.

Run `make run` to generate the server and client and execute the showcase.
After generation, `cargo run --quiet` runs it again.

For a concrete protocol, configure the client with `Config::builder().protocol(...)`:

```rust
use aws_smithy_cbor::protocol::RpcV2CborProtocol;
use multi_protocol_client::{Client, Config};

let client = Client::from_conf(
    Config::builder()
        .endpoint_url(endpoint)
        .protocol(RpcV2CborProtocol::new())
        .build(),
);
```

The executable stores different protocol implementations in `SharedClientProtocol`
and passes the runtime choice to `set_protocol(Some(protocol))`. Each resulting
client has the same generated type and operation API.

The client and wire test suites live in
`codegen-server-test/integration-tests/protocol-compat`, alongside the full service
fixture and shared models.
