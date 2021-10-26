RFC: Fine-grained timeout configuration
=======================================

> Status: RFC

For a summarized list of proposed changes, see the [Changes Checklist](#changes-checklist) section.

It is not currently possible for users of the SDK to configure a client's maximum number of retry attempts. This RFC establishes a method for users to set the number of retries to attempt when calling a service and would allow users to disable retries entirely. This RFC would introduce breaking changes to the `retry` module of the `aws-smithy-client` crate.

Terminology
-----------

- **Smithy Client**: A `aws_smithy_client::Client<C, M, R>` struct that is responsible for gluing together
  the connector, middleware, and retry policy. This is not generated and lives in the `aws-smithy-client` crate.
- **Fluent Client**: A code-generated `Client<C, M, R>` that has methods for each service operation on it.
  A fluent builder is generated alongside it to make construction easier.
- **AWS Client**: A specialized Fluent Client that defaults to using a `DynConnector`, `AwsMiddleware`,
  and `Standard` retry policy.
- **Shared Config**: An `aws_types::Config` struct that is responsible for storing shared configuration data that is used across all services. This is not generated and lives in the `aws-types` crate.
- **Service-specific Config**: A code-generated `Config` that has methods for setting service-specific configuration. Each `Config` is defined in the `config` module of its parent service. For example, the S3-specific config struct is `use`able from `aws_sdk_s3::config::Config` and re-exported as `aws_sdk_s3::Config`.

Configuring timeouts
--------------------

// TODO

Behind the scenes
-----------------

// TODO

Changes checklist
-----------------

// TODO
