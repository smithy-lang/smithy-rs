# Transport
The transport layer of smithy-rs and the Rust SDK. Our goal is support customers to bring their own HTTP stack and runtime.

## Where we are today
`aws-hyper` assembles a middleware stack with `tower`. It provides a way to use an HTTP client other than Hyper, however, it currently has a hard dependency on Hyper & Tokio. `hyper::Body` is being used directly as the body implementation for responses.

## Where we want to go
1. Extend `HttpService` to add a `sleep` method. This is required to enable runtimes other than Tokio to define how they should sleep.
2. Replace `hyper::Body` in responses with SDK Body. For now, SDKBody will probably privately wrap `hyper::Body`.
3. Merge `aws-hyper` into `aws-http`. Tokio becomes an optional feature—When the Tokio feature is opted out the "fast path" variants for the connection variants are `cfg`'d out.
4. By default, customers get a fully baked HTTP stack, but they can opt out of certain features and BYO implementation of `HttpService`.
