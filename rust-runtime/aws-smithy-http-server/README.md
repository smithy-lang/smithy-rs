# aws-smithy-http-server

Server libraries for smithy-rs generated servers.

## Event-stream runtime benchmarks

`cargo bench --quiet --bench event_streams` measures schema event serialization,
deserialization, frame encoding/decoding, modeled exceptions, and complete response-body
draining for REST JSON and RPC v2 CBOR. Run it from this crate directory.

The benchmark uses a one-time copy of generated types and schemas checked into
[`benches/fixtures/event_stream`](benches/fixtures/event_stream/README.md), with no generated
SDK dependency or codegen prerequisite. That document describes the snapshot,
measurement boundaries, and smoke-test command.

<!-- anchor_start:footer -->
This crate is part of the [AWS SDK for Rust](https://awslabs.github.io/aws-sdk-rust/) and the [smithy-rs](https://github.com/smithy-lang/smithy-rs) code generator. In most cases, it should not be used directly.
<!-- anchor_end:footer -->
