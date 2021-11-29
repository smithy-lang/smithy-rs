# aws-smithy-async

Runtime-agnostic abstractions and utilities for asynchronous code in smithy-rs.

Async runtime specific code is abstracted behind async traits, and implementations are provided via feature flag. For
now, only Tokio runtime implementations are provided.
