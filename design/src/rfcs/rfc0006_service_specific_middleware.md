RFC: Service-specific middleware
================================

> Status: [Implemented](https://github.com/smithy-lang/smithy-rs/pull/959)

For a summarized list of proposed changes, see the [Changes Checklist](#changes-checklist) section.

Currently, all services use a centralized `AwsMiddleware` that is defined in the (poorly named) `aws-hyper` crate. This
poses a number of long term risks and limitations:

1. When creating a Smithy Client directly for a given service, customers are forced to implicitly assume that the
   service uses stock `AwsMiddleware`. This prevents us from _ever_ changing the middleware stack for a service in the
   future.
2. It is impossible / impractical in the current situation to alter the middleware stack for a given service. For
   services like S3, we will almost certainly want to customize endpoint middleware in a way that is currently
   impossible.

In light of these limitations, this RFC proposes moving middleware into each generated service. `aws-inlineable` will be
used to host and test the middleware stack. Each service will then define a public `middleware` module containing their
middleware stack.

Terminology
-----------

- **Middleware**: A tower layer that augments `operation::Request -> operation::Response` for things like signing and
  endpoint resolution.
- **Aws Middleware**: A specific middleware stack that meets the requirements for AWS services.
- **Smithy Client**: A `aws_smithy_client::Client<C, M, R>` struct that is responsible for gluing together the
  connector, middleware, and retry policy. This is not generated and lives in the `aws-smithy-client` crate.
- **Fluent Client**: A code-generated `Client<C, M, R>` that has methods for each service operation on it. A fluent
  builder is generated alongside it to make construction easier.
- **AWS Client**: A specialized Fluent Client that defaults to using a `DynConnector`, `AwsMiddleware`, and `Standard`
  retry policy.
- **Shared Config**: An `aws_types::Config` struct that is responsible for storing shared configuration data that is
  used across all services. This is not generated and lives in the `aws-types` crate.
- **Service-specific Config**: A code-generated `Config` that has methods for setting service-specific configuration.
  Each `Config` is defined in the `config` module of its parent service. For example, the S3-specific config struct
  is `use`able from `aws_sdk_s3::config::Config` and re-exported as `aws_sdk_s3::Config`.

# Detailed Design

Currently, `AwsMiddleware` is defined in `aws-hyper`. As part of this change, an `aws-inlineable` dependency will be
added containing code that is largely identical. This will be exposed in a public `middleware` module in all generated
services. At some future point, we could even expose a baseline set of default middleware for whitelabel Smithy services
to make them easier to use out-of-the-box.

The `ClientGenerics` parameter of the `AwsFluentClientGenerator` will be updated to become a `RuntimeType`, enabling
loading the type directly. This has the advantage of making it fairly easy to do per-service middleware stacks since we
can easily configure `AwsFluentClientGenerator` to insert different types based on the service id.

# Changes Checklist

- [x] Move aws-hyper into aws-inlineable. Update comments as needed including with a usage example about how customers can augment it.
- [x] Refactor `ClientGenerics` to contain a RuntimeType instead of a string and configure. Update `AwsFluentClientDecorator`.
- [x] Update all code and examples that use `aws-hyper` to use service-specific middleware.
- [x] Push an updated README to aws-hyper deprecating the package, explaining what happened. Do _not_ yank previous versions since those will be relied on by older SDK versions.
