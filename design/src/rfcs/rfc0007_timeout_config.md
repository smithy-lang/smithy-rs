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

// TODO clean this up

- Middleware: almost means the same thing as layer but is the only thing allowed to contain a `DispatchService`
- Connector: overloaded word,
  - (esp. `DynConnector`) generally refers to a service that handles HTTP. HTTP is the lowest level we handle
    - Dyn means that we're erasing the inner type
  - `Connector` has its own meaning in `hyper`. Something that implements the `Connect` trait
- `Connection` is also a hyper term representing a low level protocol talky thing
- Service: The lowest level of abstraction, cares about actual data
- Layer: an abstraction for wrapping services inside one another. Makes it easy to define services that can wrap other services that they have no knowledge of. Layers are Middleware
    A layer is the ability to define how one service gets wrapped around another. You can wrap services without the layer abstraction but then defining the types gets tough and is rigid. A higher-order service. Only used to define the relationships between services
when creating layer structs, they don't contain services but they do create them. synthesis of the inputs, a third object.
- Stage: not tower-specific. Functions as a way of mapping requests right now. Timeouts wont be a stage because we don't want to give them the ability to handle responses because it would make them too complex
- Stack: higher order abstraction over a stack

ParseResponse is the Bizarro version of MapRequest

Configuring timeouts
--------------------

This section will define the kinds of timeouts that will be configurable and the ways that a user could configure them.

- **Connect Timeout**: A limit on the amount of time after making an initial connect attempt on a socket to complete the
  connect-handshake.
    - _TODO: the runtime is based on Hyper which reuses connection and doesn't currently have a way of guaranteeing that
      a fresh connection will be use for a given request._
- **TLS Negotiation Timeout**: A limit on the amount of time a TLS handshake takes from when the CLIENT HELLO message is
  sent to the time the client and server have fully negotiated ciphers and exchanged keys.
    - _QUESTION: Our HTTP client supports multiple TLS implementations. Is there any reason that that would make
      implementing this feature difficult?_
- **Time to First Byte Timeout**: A limit on the amount of time an application takes to attempt to read the first byte over
  an established, open connection after write request.
  - _QUESTION: Is this the same as a "write timeout"? If yes, should we refer to it by that name? Also, if yes, should we not also support "read timeouts" for completeness' sake?_
- **HTTP Request Timeout For A Single Request**: A limit on the amount of time it takes for the first byte to be sent over
  an established, open connection and when the last byte is received from the service.
- **HTTP Request Timeout For Multiple Attempts**: This timeout acts like the previous timeout but constrains the total time
  it takes to make a request plus any retries.
    - _NOTE: In a way, this is already possible in that users are free to race requests against timer futures with
      the [futures::future::select] macro or to use [tokio::time::timeout]. See relevant discussion in [hyper#1097]_

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

Prior Art
---------

- [hjr3/hyper-timeout] is a `Connector` for hyper that enables setting connect, read, and write timeouts
- [sfackler/tokio-io-timeout] provides timeouts for tokio IO operations. Used within `hyper-timeout`.
- [tokio::time::sleep_until] creates a `Future` that completes after some time has elapsed. Used within `tokio-io-timeout`.

Behind the scenes
-----------------

// TODO This is just Zelda thinking out loud and working backwards.

Timeouts are achieved by racing a future against a `tokio::time::Sleep` future. The question, then, is "how can I create a future that represents a condition I want to watch for?". For example, in the case of a `ConnectTimeout`, how do we watch an ongoing request to see if it's completed the connect-handshake?

```rust
#[tokio::main]
async fn main() {
  let req = make_fake_request();
  todo!("???")
}

fn make_fake_request() -> Future<Output = FakeRequest> {
  todo!("???")
}
```

Changes checklist
-----------------

// TODO

[tokio::time::timeout]: https://docs.rs/tokio/1.12.0/tokio/time/fn.timeout.html
[futures::future::select]: https://docs.rs/futures/0.3.17/futures/future/fn.select.html
[Retry Behavior Configuration]: ./rfc0004_retry_behavior.md
[hyper#1097]: https://github.com/hyperium/hyper/issues/1097
[hjr3/hyper-timeout]: https://github.com/hjr3/hyper-timeout
[sfackler/tokio-io-timeout]: https://github.com/sfackler/tokio-io-timeout
