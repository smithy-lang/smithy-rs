# aws-smithy-http-client

HTTP client abstractions for generated smithy clients.

## Connection test harness

The `wire-mock` feature exposes a deterministic connection-level test harness at
`test_util::wire::connection`. Each accepted socket receives one complete script:

```rust,no_run
use aws_smithy_http_client::test_util::wire::connection::{
    ConnectionTestHarness, EndpointPlan, Http1Response, Http1Script,
};
use std::net::{IpAddr, Ipv4Addr};

# async fn example() -> Result<(), Box<dyn std::error::Error>> {
let harness = ConnectionTestHarness::builder()
    .endpoint(
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        EndpointPlan::unbounded(Http1Script::serve(
            Http1Response::ok().body("ok"),
        )),
    )
    .build()
    .await?;

let endpoint = harness.endpoint_url();
// Configure the client with `endpoint` and `harness.dns_resolver()`.

harness.shutdown().await?;
# Ok(())
# }
```

<!-- anchor_start:footer -->
This crate is part of the [AWS SDK for Rust](https://awslabs.github.io/aws-sdk-rust/) and the [smithy-rs](https://github.com/smithy-lang/smithy-rs) code generator. In most cases, it should not be used directly.
<!-- anchor_end:footer -->
