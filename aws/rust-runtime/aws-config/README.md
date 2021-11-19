# aws-config

AWS SDK config and credential provider implementations.

**Please Note: The SDK is currently released as an alpha and is intended strictly for feedback purposes only. Do not use this SDK for production workloads.**

Config provides a way to keep track of the configurations of all the Amazon Web Services resources associated with your Amazon Web Services account. You can use Config to get the current and historical configurations of each Amazon Web Services resource and also to get information about the relationship between the resources. An Amazon Web Services resource can be an Amazon Compute Cloud (Amazon EC2) instance, an Elastic Block Store (EBS) volume, an elastic network Interface (ENI), or a security group. For a complete list of resources currently supported by Config, see [Supported Amazon Web Services resources].

You can access and manage Config through the Amazon Web Services Management Console, the Amazon Web Services Command Line Interface (Amazon Web Services CLI), the Config API, or the Amazon Web Services SDKs for Config. This reference guide contains documentation for the Config API and the Amazon Web Services CLI commands that you can use to manage Config. The Config API uses the Signature Version 4 protocol for signing requests. For more information about how to sign a request with this protocol, see [Signature Version 4 Signing Process]. For detailed information about Config features and their associated actions or commands, as well as how to work with Amazon Web Services Management Console, see [What Is AWS Config?] in the _Config Developer Guide_.

## Getting Started

    Examples are available for many services and operations, check out the [examples folder in GitHub].

The SDK provides one crate per AWS service. You must add Tokio as a dependency within your Rust project to execute asynchronous code. To add aws-sdk-config to your project, add the following to your Cargo.toml file:

```toml
[dependencies]
aws-config = "0.0.26-alpha"
aws-sdk-config = "0.0.26-alpha"
tokio = { version = "1", features = ["full"] }
```

## Using the SDK

Until the SDK is released, we will be adding information about using the SDK to the [Guide]. Feel free to suggest additional sections for the guide by opening an issue and describing what you are trying to do.

## Getting Help

- [GitHub discussions] - For ideas, RFCs & general questions
- [GitHub issues] – For bug reports & feature requests
- [Generated Docs] (latest version)
- [Usage examples]

## License

This project is licensed under the Apache-2.0 License.

[Supported Amazon Web Services resources]: https://docs.aws.amazon.com/config/latest/developerguide/resource-config-reference.html#supported-resources
[Signature Version 4 Signing Process]: https://docs.aws.amazon.com/general/latest/gr/signature-version-4.html
[What Is AWS Config?]: https://docs.aws.amazon.com/config/latest/developerguide/WhatIsConfig.html
[examples folder in GitHub]: https://github.com/awslabs/aws-sdk-rust/tree/main/sdk/examples
[Tokio]: https://crates.io/crates/tokio
[Guide]: https://github.com/awslabs/aws-sdk-rust/blob/main/Guide.md
[GitHub discussions]: https://github.com/awslabs/aws-sdk-rust/discussions
[GitHub issues]: https://github.com/awslabs/aws-sdk-rust/issues/new/choose
[Generated Docs]: https://awslabs.github.io/aws-sdk-rust/
[Usage examples]: https://github.com/awslabs/aws-sdk-rust/tree/main/sdk/examples
