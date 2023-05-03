# Design Overview

The AWS Rust SDK aims to provide an official, high quality & complete interface to AWS services. We plan to eventually use the CRT to provide signing & credential management. The Rust SDK will provide first-class support for the CRT as well as [Tokio ](https://tokio.rs/) & [Hyper](https://hyper.rs). The Rust SDK empowers advanced customers to bring their own HTTP/IO implementations.

Our design choices are guided by our [Tenets](./tenets.md).

## Acknowledgments

The design builds on the learnings, ideas, hard work, and GitHub issues of the 142 Rusoto contributors & thousands of users who built this first and learned the hard way.

## External API Overview

The Rust SDK is "modular" meaning that each AWS service is its own crate. Each crate provides two layers to access the service:
1. The "fluent" API. For most use cases, a high level API that ties together connection management and serialization will be the quickest path to success.

```rust,ignore
#[tokio::main]
async fn main() {
    let client = dynamodb::Client::from_env();
    let tables = client
        .list_tables()
        .limit(10)
        .send()
        .await.expect("failed to load tables");
}
```

2. The "low-level" API: It is also possible for customers to assemble the pieces themselves. This offers more control over operation construction & dispatch semantics:

```rust,ignore
#[tokio::main]
async fn main() {
    let conf = dynamodb::Config::builder().build();
    let conn = aws_hyper::Client::https();
    let operation = dynamodb::ListTables::builder()
        .limit(10)
        .build(&conf)
        .expect("invalid operation");
    let tables = conn.call(operation).await.expect("failed to list tables");
}
```

The Fluent API is implemented as a thin wrapper around the core API to improve ergonomics.

## Internals
The Rust SDK is built on Tower Middleware, Tokio & Hyper. We're continuing to iterate on the internals to enable running the AWS SDK in other executors & HTTP stacks. As an example, you can see a demo of adding `reqwest` as a custom HTTP stack to gain access to its HTTP Proxy support!

For more details about the SDK internals see [Operation Design](transport/operation.md)

## Code Generation
The Rust SDK is code generated from Smithy models, using Smithy codegeneration utilities. The Code generation is written in Kotlin. More details can be found in the [Smithy](./smithy/overview.md) section.
