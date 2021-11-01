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

There's a lot of terminology to define so I've broken it up into two sections

### General terms

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
  is `use`able from `aws_sdk_s3::config::Config` and re-exported as `aws_sdk_s3::Config`. In this case, "service" refers to an AWS offering like S3.

### HTTP stack terms

- **Service**: A trait defined in the [`tower-service` crate][tower_service::Service]. The lowest level of abstraction we deal with when making HTTP requests. Services act directly on data to transform and modify that data. A Service is what eventually turns a request into a response.
- **Layer**: Layers are a higher-order abstraction over services that is used to compose multiple services together, creating a new service from that combination. Nothing prevents us from manually wrapping services withing services, but Layers allow us to do it in a maintainable, flexible, and generic manner. Layers don't directly act on data but instead can wrap an existing service with additional functionality, creating a new service.. Layers can be thought of as middleware.
- **Middleware**: a term with several meanings,
  - Generically speaking, middlewares are similar to Services and Layers in that they modify requests and responses.
  - In the SDK, "Middleware" refers to a layer that can be wrapped around a `DispatchService`. In practice, this means that the resulting `Service` (and the inner service) must meet the bound `T: where T: Service<operation::Request, Response=operation::Response, Error=SendOperationError>`.
  - The most notable example of a Middleware is the [AwsMiddleware]. Other notable examples include [MapRequest], [AsyncMapRequest], and [ParseResponse].
- **DispatchService**: The innermost part of a group of nested services. The Service that actually makes an HTTP call on behalf of a request. Responsible for parsing success and error responses.
- **Connector**: a term with several meanings,
  - [DynConnector]s are Services with their specific type erased so that we can do dynamic dispatch.
  - A term from `hyper` for any object that implements the [Connect] trait. Really just an alias for [tower_service::Service]. Sometimes referred to as a `Connection`.
- **Stage**: A form of middleware that's not related to `tower`. These currently function as a way of transforming requests. We don't want to implement Timeouts as stages because we don't want to give them the ability to handle responses. This is because it would introduce lots of new complexity that we aren't yet ready to take on but this may change in the future.
- **Stack**: higher order abstraction over Layers defined in the [tower crate][tower::layer::util::Stack] e.g. Layers wrap services in one another and Stacks wrap layers within one another.

### Timeout terms

- **Connect Timeout**: A limit on the amount of time after making an initial connect attempt on a socket to complete the
  connect-handshake.
    - _TODO: the runtime is based on Hyper which reuses connection and doesn't currently have a way of guaranteeing that
      a fresh connection will be use for a given request._
- **TLS Negotiation Timeout**: A limit on the amount of time a TLS handshake takes from when the CLIENT HELLO message is
  sent to the time the client and server have fully negotiated ciphers and exchanged keys.
    - _Note: Our HTTP client supports multiple TLS implementations. We'll likely have to implement this feature once per library._
- **Time to First Byte Timeout**: _Sometimes referred to as a "read timeout."_ A limit on the amount of time an application takes to attempt to read the first byte over
  an established, open connection after write request.
- **HTTP Request Timeout For A Single Request**: A limit on the amount of time it takes for the first byte to be sent over
  an established, open connection and when the last byte is received from the service.
- **HTTP Request Timeout For Multiple Attempts**: This timeout acts like the previous timeout but constrains the total time
  it takes to make a request plus any retries.
    - _NOTE: In a way, this is already possible in that users are free to race requests against timer futures with
      the [futures::future::select] macro or to use [tokio::time::timeout]. See relevant discussion in [hyper#1097]_

Configuring timeouts
--------------------

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

Timeouts are achieved by racing a future against a `tokio::time::Sleep` future. The question, then, is "how can I create a future that represents a condition I want to watch for?". For example, in the case of a `ConnectTimeout`, how do we watch an ongoing request to see if it's completed the connect-handshake? Our current stack of Middleware acts on requests at different levels of granularity. The timeout Middlewares will be no different.

### Middlewares for AWS Client requests

_View [AwsMiddleware] in GitHub_

```rust
#[derive(Debug, Default)]
#[non_exhaustive]
pub struct AwsMiddleware;
impl<S> tower::Layer<S> for AwsMiddleware {
  type Service = <AwsMiddlewareStack as tower::Layer<S>>::Service;

  fn layer(&self, inner: S) -> Self::Service {
    let credential_provider = AsyncMapRequestLayer::for_mapper(CredentialsStage::new());
    let signer = MapRequestLayer::for_mapper(SigV4SigningStage::new(SigV4Signer::new()));
    let endpoint_resolver = MapRequestLayer::for_mapper(AwsEndpointStage);
    let user_agent = MapRequestLayer::for_mapper(UserAgentStage::new());
    ServiceBuilder::new()
            .layer(endpoint_resolver)
            .layer(user_agent)
            .layer(credential_provider)
            .layer(signer)
            .service(inner)
  }
}
```

The above code is only included for context. This RFC doesn't define any timeouts specific to AWS so `AwsMiddleware` won't require any changes.

### Middlewares for Smithy Client requests

_View [aws_smithy_client::Client::call_raw] in GitHub_

```rust
impl<C, M, R> Client<C, M, R>
  where
          C: bounds::SmithyConnector,
          M: bounds::SmithyMiddleware<C>,
          R: retry::NewRequestPolicy,
{
  // ...other methods omitted
  pub async fn call_raw<O, T, E, Retry>(
    &self,
    input: Operation<O, Retry>,
  ) -> Result<SdkSuccess<T>, SdkError<E>>
    where
            R::Policy: bounds::SmithyRetryPolicy<O, T, E, Retry>,
            bounds::Parsed<<M as bounds::SmithyMiddleware<C>>::Service, O, Retry>:
            Service<Operation<O, Retry>, Response=SdkSuccess<T>, Error=SdkError<E>> + Clone,
  {
    let connector = self.connector.clone();
    let mut svc = ServiceBuilder::new()
            // Create a new request-scoped policy
            .retry(self.retry_policy.new_request_policy())
            .layer(ParseResponseLayer::<O, Retry>::new())
            // These layers can be considered as occurring in order. That is, first invoke the
            // customer-provided middleware, then dispatch dispatch over the wire.
            .layer(&self.middleware)
            .layer(DispatchLayer::new())
            .service(connector);
    svc.ready().await?.call(input).await
  }
}
```

The Smithy Client creates a new service to handle each request it sends. Each service is constructed with the following features:

- A method `retry` is used to pass in retry-related configuration such as the maximum number of times to attempt the request.
- A Layer for transforming responses into operation-specific outputs or errors. The `O` generic parameter of `input` is what decides exactly how the transformation is implemented.
- The middleware stack that was included during Client creation. In the case of the AWS SDK, this would be `AwsMiddleware`.
- The `DispatchLayer`, a layer for transforming an `http::Request` into an `operation::Request`. It's also responsible for re-attaching the property bag from the Operation that triggered the request.
- The innermost Service is a `DynConnector` wrapping a `hyper` client (which one depends on the TLS implementation that was chosen.)

Timeouts will be implemented in the following places:

- HTTP request timeout for multiple requests will be implemented as the outermost Layer in `call_raw`.
- HTTP request timeout for a single request will be implemented within `RetryHandler::retry`.
- Time to first byte, TLS negotiation, and connect timeouts will be implemented within the central `hyper` connector.

Changes checklist
-----------------

// TODO

<!--- Links -->

[tokio::time::timeout]: https://docs.rs/tokio/1.12.0/tokio/time/fn.timeout.html
[futures::future::select]: https://docs.rs/futures/0.3.17/futures/future/fn.select.html
[Retry Behavior Configuration]: ./rfc0004_retry_behavior.md
[hyper#1097]: https://github.com/hyperium/hyper/issues/1097
[hjr3/hyper-timeout]: https://github.com/hjr3/hyper-timeout
[sfackler/tokio-io-timeout]: https://github.com/sfackler/tokio-io-timeout
[tower_service::Service]: https://docs.rs/tower-service/0.3.1/tower_service/trait.Service.html
[AwsMiddleware]: https://github.com/awslabs/smithy-rs/blob/1aa59693eed10713dec0f3774a8a25ca271dbf39/aws/rust-runtime/aws-hyper/src/lib.rs#L29
[MapRequest]: https://github.com/awslabs/smithy-rs/blob/841f51113fb14e2922793951ce16bda3e16cb51f/rust-runtime/aws-smithy-http-tower/src/map_request.rs#L122
[AsyncMapRequest]: https://github.com/awslabs/smithy-rs/blob/841f51113fb14e2922793951ce16bda3e16cb51f/rust-runtime/aws-smithy-http-tower/src/map_request.rs#L42
[ParseResponse]: https://github.com/awslabs/smithy-rs/blob/841f51113fb14e2922793951ce16bda3e16cb51f/rust-runtime/aws-smithy-http-tower/src/parse_response.rs#L27
[DynConnect]: https://github.com/awslabs/smithy-rs/blob/1aa59693eed10713dec0f3774a8a25ca271dbf39/rust-runtime/aws-smithy-client/src/erase.rs#L139
[Connect]: https://docs.rs/hyper/0.14.14/hyper/client/connect/trait.Connect.html
[tower::layer::util::Stack]: https://docs.rs/tower/0.4.10/tower/layer/util/struct.Stack.html
[aws_smithy_client::Client::call_raw]: https://github.com/awslabs/smithy-rs/blob/841f51113fb14e2922793951ce16bda3e16cb51f/rust-runtime/aws-smithy-client/src/lib.rs#L175
