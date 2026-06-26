# aws-smithy-http-client

HTTP client abstractions for generated smithy clients.

## Testing

### Connection pool integration tests

```sh
cargo test --features wire-mock,default-client --test connection_harness_test
```

Some tests simulate multiple S3 IPs by binding to different loopback addresses (`127.0.0.1`, `127.0.0.2`, etc.) on the same port.
On Linux this works out of the box. On macOS, loopback aliases must be configured first:

```sh
sudo ifconfig lo0 alias 127.0.0.2
sudo ifconfig lo0 alias 127.0.0.3
```

These aliases do not persist across reboots. Without them, multi-IP tests will be skipped.

<!-- anchor_start:footer -->
This crate is part of the [AWS SDK for Rust](https://awslabs.github.io/aws-sdk-rust/) and the [smithy-rs](https://github.com/smithy-lang/smithy-rs) code generator. In most cases, it should not be used directly.
<!-- anchor_end:footer -->
