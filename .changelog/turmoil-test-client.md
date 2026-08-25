---
applies_to: ["client"]
authors: ["jasgin"]
references: ["smithy-rs#4806"]
breaking: false
new_feature: true
bug_fix: false
---
Add a test-only `test_util::turmoil_client` to `aws-smithy-http-client` (behind the new mutually-exclusive `turmoil-06` / `turmoil-07` features, selecting the `turmoil` 0.6 or 0.7 major version) that drives the real HTTP client over the [turmoil](https://docs.rs/turmoil) discrete-event network simulator. Call `turmoil_client(resolver, port)` to get a `SharedHttpClient`; it runs through the same connection pool, connect/read timeout, and error-classification stack as the production client, so only the transport is replaced. TLS is not layered on top (the transport is plaintext). All hyper IO plumbing (`TokioIo`, `hyper::rt::{Read, Write}`, `Connection`) is crate-private, so consumers never depend on hyper internals.
