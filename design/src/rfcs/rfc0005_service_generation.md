RFC: Smithy Rust Service Framework
==================================

> Status: RFC

The Rust Smithy Framework is a full-fledged service framework whose main
responsibility is to handle request lifecycles from beginning to end. It takes
care of input de-serialization, operation execution, output serialization,
error handling, and provides facilities to fulfill the requirements below.

Requirements
------------

### Smithy model-driven code generation

Server side code is generated from Smithy models and implements operations,
input and output structures, and errors defined in the service model.

### Performance

This new framework is built with performance in mind. It refrains from
allocating memory when not needed and try to use a majority of
[borrowed](https://doc.rust-lang.org/std/borrow/trait.Borrow.html) types,
handling their memory lifetimes so that a request body can be stored in memory
only once and not
[cloned](https://doc.rust-lang.org/std/clone/trait.Clone.html) if possible.

The code is implemented on solid and widely used foundations. It uses
[Hyper](https://hyper.rs/) to handle the HTTP protocol, the
[Tokio](https://tokio.rs/) ecosystem for asynchronous (non-blocking) operations
and [Tower](https://docs.rs/tower/) to implement middleware such as timeouts,
rate limiting, retries, and more. CPU intensive operations are scheduled on a
separated thread-pool to avoid blocking the event loop.

It uses Tokio [axum](https://tokio.rs/blog/2021-07-announcing-axum), an HTTP
framework built on top of the technologies mentioned above which handles
routing, request extraction, response building, and workers lifecycle. Axum is
a relatively thin layer on top of Hyper and adds very little overhead, so its
[performance is comparable](https://github.com/programatik29/rust-web-benchmarks/blob/master/results/hello-world.md)
to Hyper.

The framework should be able to let customers use the built-in HTTP server or
select other transport implementations that can be more performant or better
suited than HTTP for their use case.

### Extensibility

We want to deliver an extensible framework that can plugin components possibly
during code generation and at runtime for specific scenarios that cannot be
covered during generation. These components are developed using a standard
[interface](https://doc.rust-lang.org/book/ch10-02-traits.html) provided by the
framework itself.

### Observability

Being able to report and trace the status of the service is vital for the
success of any product. The framework is integrated with tracing and allow
non-blocking I/O through the asynchronous
[tracing appender](https://tracing.rs/tracing_appender/index.html#non-blocking-writer).

Metrics and logging are built with extensibility in mind, allowing customers to
plug their own handlers following a well defined interface provided by the
framework.

### Client generation

Client generation is deferred to the various Smithy implementations.

### Benchmarking

Benchmarking the framework is key and customers can't use anything that
compromises the fundamental business objectives of latency and performance.

### Model validation

The generated service code is responsible for validating the model constraints of input structures.
