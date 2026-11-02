# aws-smithy-http-server-python

Server libraries for smithy-rs generated servers, targeting pure Python business logic.

HTTP and TLS servers limit accepted connections to 16,384 per worker by default.
Connections performing TLS handshakes count toward the same limit. Excess
connections wait in the socket backlog until a slot is released, and HTTP/1
request headers must complete within 60 seconds.

Regenerate the Python SDK to configure a positive per-worker limit:

```python
app.run(address="127.0.0.1", port=13734, workers=2, max_connections=1024)
```

The total connection capacity scales with the number of worker processes. The
`backlog` argument configures the socket's pending connection queue separately.
SDKs generated before `max_connections` was introduced receive the default
protection when rebuilt with this runtime.

<!-- anchor_start:footer -->
This crate is part of the [AWS SDK for Rust](https://awslabs.github.io/aws-sdk-rust/) and the [smithy-rs](https://github.com/smithy-lang/smithy-rs) code generator. In most cases, it should not be used directly.
<!-- anchor_end:footer -->
