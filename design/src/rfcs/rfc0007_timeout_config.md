RFC: Fine-grained timeout configuration
=======================================

> Status: RFC

For a summarized list of proposed changes, see the [Changes Checklist](#changes-checklist) section.

It is not currently possible for users of the SDK to configure a client's maximum number of retry attempts. This RFC
establishes a method for users to set the number of retries to attempt when calling a service and would allow users to
disable retries entirely. This RFC would introduce breaking changes to the `retry` module of the `aws-smithy-client`
crate.

Terminology
-----------

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

Configuring timeouts
--------------------

This section will define the kinds of timeouts that will be configurable and the ways that a user could configure them.

- Connect Timeout: A limit on the amount of time after making an initial connect attempt on a socket to complete the
  connect-handshake.
    - _TODO: the runtime is based on Hyper which reuses connection and doesn't currently have a way of guaranteeing that
      a fresh connection will be use for a given request._
- TLS Negotiation Timeout: A limit on the amount of time a TLS handshake takes from when the CLIENT HELLO message is
  sent to the time the client and server have fully negotiated ciphers and exchanged keys.
    - _QUESTION: Our HTTP client supports multiple TLS implementations. Is there any reason that that would make
      implementing this feature difficult?_
- Time to First Byte Timeout: A limit on the amount of time an application takes to attempt to read the first byte over
  an established, open connection after write request.
- HTTP Request Timeout For A Single Request: A limit on the amount of time it takes for the first byte to be sent over
  an established, open connection and when the last byte is received from the service.
- HTTP Request Timeout For Multiple Attempts: This timeout acts like the previous timeout but constrains the total time
  it takes to make a request plus any retries.
    - _NOTE: In a way, this is already possible in that users are free to race requests against timer futures with
      the [futures::future::select] macro or to use [tokio::time::timeout]_

Just like with [Retry Behavior Configuration], these settings can be configured in several places and have the same
precedence rules _(paraphrased here for clarity)_.

- By calling timeout configuration methods when building a service-specific config
- By calling the timeout configuration methods when building a shared config
- By setting timeout configuration environment variables
- By setting timeout configuration variables in the AWS config of the active profile

The above list is in order of decreasing precedence e.g. configuration set in an app will override values from
environment variables.

### Configuration options

The table below details the specific ways each timeout can be configured. In all cases, valid values are non-negative floats representing the number of seconds before a timeout is triggered.

| Timeout                       | Environment Variable         | AWS Config Variable      | Builder Method           |
| ----------------------------- | ---------------------------- | ------------------------ | ------------------------ |
| Connect                       | AWS_CONNECT_TIMEOUT          | connect_timeout          | connect_timeout          |
| TLS Negotiation               | AWS_TLS_NEGOTIATION_TIMEOUT  | tls_negotiation_timeout  | tls_negotiation_timeout  |
| Time To First Byte            | AWS_WRITE_TIMEOUT            | write_timeout            | write_timeout            |
| HTTP Request - single attempt | AWS_API_CALL_ATTEMPT_TIMEOUT | api_call_attempt_timeout | api_call_attempt_timeout |
| HTTP Request - all attempts   | AWS_API_CALL_TIMEOUT         | api_call_timeout         | api_call_timeout         |

### SDK-specific defaults set by AWS service teams

_QUESTION: How does the SDK currently handle these defaults?_

Behind the scenes
-----------------

// TODO

Changes checklist
-----------------

// TODO

[tokio::time::timeout]: https://docs.rs/tokio/1.12.0/tokio/time/fn.timeout.html
[futures::future::select]: https://docs.rs/futures/0.3.17/futures/future/fn.select.html
[Retry Behavior Configuration]: ./rfc0004_retry_behavior.md
