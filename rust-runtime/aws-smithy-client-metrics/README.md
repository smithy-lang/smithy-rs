# aws-smithy-client-metrics

Per-request client metrics for [smithy-rs](https://github.com/smithy-lang/smithy-rs) clients, including the AWS SDK for Rust, emitted through [metrique](https://github.com/awslabs/metrique).

Install `ClientMetrics` once on a client. By default, SDK call fields are added to the current request's metrique entry; repeated calls with the same service, qualifier and operation keep the last call. If no request entry is in scope, the crate logs a rate-limited warning and writes a standalone entry instead.

```rust,ignore
use aws_smithy_client_metrics::ClientMetrics;

let config = aws_sdk_s3::config::Builder::from(&sdk_config)
    .interceptor(ClientMetrics::new())
    .build();
```

See the crate documentation for the emitted fields and configuration options.

<!-- anchor_start:footer -->
This crate is part of the [AWS SDK for Rust](https://awslabs.github.io/aws-sdk-rust/) and the [smithy-rs](https://github.com/smithy-lang/smithy-rs) code generator.
<!-- anchor_end:footer -->
