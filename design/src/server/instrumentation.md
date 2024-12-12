# Instrumentation

A Smithy Rust server uses the [`tracing`](https://github.com/tokio-rs/tracing) crate to provide instrumentation. The customer is responsible for setting up a [`Subscriber`](https://docs.rs/tracing/latest/tracing/subscriber/trait.Subscriber.html) in order to ingest and process [events](https://docs.rs/tracing/latest/tracing/struct.Event.html) - Smithy Rust makes no prescription on the choice of `Subscriber`. Common choices might include:

- [`tracing_subscriber::fmt`](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/fmt/index.html) for printing to `stdout`.
- [`tracing-log`](https://crates.io/crates/tracing-log) to providing compatibility with the [`log`](https://crates.io/crates/log).

Events are emitted and [spans](https://docs.rs/tracing/latest/tracing/struct.Span.html) are opened by the `aws-smithy-http-server`, `aws-smithy-http-server-python`, and generated crate. The [default](https://docs.rs/tracing/latest/tracing/struct.Metadata.html) [target](https://docs.rs/tracing/latest/tracing/struct.Metadata.html#method.target) is always used

> The tracing macros default to using the module path where the span or event originated as the target, but it may be overridden.

and therefore spans and events be filtered using the [`EnvFilter`](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html) and/or [`Targets`](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/targets/struct.Targets.html) filters with crate and module paths.

For example,

```bash
RUST_LOG=aws_smithy_http_server=warn,aws_smithy_http_server_python=error
```

and

```rust,ignore
# extern crate tracing_subscriber;
# extern crate tracing;
# use tracing_subscriber::filter;
# use tracing::Level;
let filter = filter::Targets::new().with_target("aws_smithy_http_server", Level::DEBUG);
```

In general, Smithy Rust is conservative when using high-priority log levels:

- ERROR
  - Fatal errors, resulting in the termination of the service.
  - Requires immediate remediation.
- WARN
  - Non-fatal errors, resulting in incomplete operation.
  - Indicates service misconfiguration, transient errors, or future changes in behavior.
  - Requires inspection and remediation.
- INFO
  - Informative events, which occur inside normal operating limits.
  - Used for large state transitions, e.g. startup/shutdown.
- DEBUG
  - Informative and sparse events, which occur inside normal operating limits.
  - Used to debug coarse-grained progress of service.
- TRACE
  - Informative and frequent events, which occur inside normal operating limits.
  - Used to debug fine-grained progress of service.

## Spans over the Request/Response lifecycle

Smithy Rust is built on top of [`tower`](https://github.com/tower-rs/tower), which means that middleware can be used to encompass different periods of the lifecycle of the request and response and identify them with a span.

An open-source example of such a middleware is [`TraceLayer`](https://docs.rs/tower-http/latest/tower_http/trace/struct.TraceLayer.html) provided by the [`tower-http`](https://docs.rs/tower-http/latest/tower_http/) crate.

Smithy provides an out-the-box middleware which:

- Opens a DEBUG level span, prior to request handling, including the operation name and request URI and headers.
- Emits a DEBUG level event, after to request handling, including the response headers and status code.

This is enabled via the `instrument` method provided by the `aws_smithy_http_server::instrumentation::InstrumentExt` trait.

```rust,no_run
# extern crate aws_smithy_http_server;
# extern crate pokemon_service_server_sdk;
# use pokemon_service_server_sdk::{operation_shape::GetPokemonSpecies, input::*, output::*, error::*};
# let handler = |req: GetPokemonSpeciesInput| async { Result::<GetPokemonSpeciesOutput, GetPokemonSpeciesError>::Ok(todo!()) };
use aws_smithy_http_server::{
  instrumentation::InstrumentExt,
  plugin::{IdentityPlugin, HttpPlugins}
};
# use aws_smithy_http_server::protocol::rest_json_1::{RestJson1, router::RestRouter};
# use aws_smithy_http_server::routing::{Route, RoutingService};
use pokemon_service_server_sdk::{PokemonServiceConfig, PokemonService};

let http_plugins = HttpPlugins::new().instrument();
let config = PokemonServiceConfig::builder().http_plugin(http_plugins).build();
let app = PokemonService::builder(config)
  .get_pokemon_species(handler)
  /* ... */
  .build()
  .unwrap();
# let app: PokemonService<RoutingService<RestRouter<Route>, RestJson1>>  = app;
```

<!-- TODO: Link to it when the logging module is no longer `#[doc(hidden)]` -->

### Example

The Pokémon service example, located at `/examples/pokemon-service`, sets up a `tracing` `Subscriber` as follows:

```rust,ignore
# extern crate tracing_subscriber;
use tracing_subscriber::{prelude::*, EnvFilter};

/// Setup `tracing::subscriber` to read the log level from RUST_LOG environment variable.
pub fn setup_tracing() {
    let format = tracing_subscriber::fmt::layer().pretty();
    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new("info"))
        .unwrap();
    tracing_subscriber::registry().with(format).with(filter).init();
}
```

Running the Pokémon service example using

```bash
RUST_LOG=aws_smithy_http_server=debug,pokemon_service=debug cargo r
```

and then using `cargo t` to run integration tests against the server, yields the following logs:

```text
  2022-09-27T09:13:35.372517Z DEBUG aws_smithy_http_server::instrumentation::service: response, headers: {"content-type": "application/json", "content-length": "17"}, status_code: 200 OK
    at /smithy-rs/rust-runtime/aws-smithy-http-server/src/logging/service.rs:47
    in aws_smithy_http_server::instrumentation::service::request with operation: get_server_statistics, method: GET, uri: /stats, headers: {"host": "localhost:13734"}

  2022-09-27T09:13:35.374104Z DEBUG pokemon_service: attempting to authenticate storage user
    at pokemon-service/src/lib.rs:184
    in aws_smithy_http_server::instrumentation::service::request with operation: get_storage, method: GET, uri: /pokedex/{redacted}, headers: {"passcode": "{redacted}", "host": "localhost:13734"}

  2022-09-27T09:13:35.374152Z DEBUG pokemon_service: authentication failed
    at pokemon-service/src/lib.rs:188
    in aws_smithy_http_server::instrumentation::service::request with operation: get_storage, method: GET, uri: /pokedex/{redacted}, headers: {"passcode": "{redacted}", "host": "localhost:13734"}

  2022-09-27T09:13:35.374230Z DEBUG aws_smithy_http_server::instrumentation::service: response, headers: {"content-type": "application/json", "x-amzn-errortype": "NotAuthorized", "content-length": "2"}, status_code: 401 Unauthorized
    at /smithy-rs/rust-runtime/aws-smithy-http-server/src/logging/service.rs:47
    in aws_smithy_http_server::instrumentation::service::request with operation: get_storage, method: GET, uri: /pokedex/{redacted}, headers: {"passcode": "{redacted}", "host": "localhost:13734"}
```

## Interactions with Sensitivity

Instrumentation interacts with Smithy's [sensitive trait](https://awslabs.github.io/smithy/2.0/spec/documentation-traits.html#sensitive-trait).

> Sensitive data MUST NOT be exposed in things like exception messages or log output. Application of this trait SHOULD NOT affect wire logging (i.e., logging of all data transmitted to and from servers or clients).

For this reason, Smithy runtime will never use `tracing` to emit events or open spans that include any sensitive data. This means that the customer can ingest all logs from `aws-smithy-http-server` and `aws-smithy-http-server-*` without fear of violating the sensitive trait.

The Smithy runtime will not, and cannot, prevent the customer violating the sensitive trait within the operation handlers and custom middleware. It is the responsibility of the customer to not violate the sensitive contract of their own model, care must be taken.

Smithy shapes can be sensitive while being coupled to the HTTP request/responses via the [HTTP binding traits](https://awslabs.github.io/smithy/2.0/spec/http-bindings.html). This poses a risk when ingesting events which naively capture request/response information. The instrumentation middleware provided by Smithy Rust respects the sensitive trait and will replace sensitive data in its span and event with `{redacted}`. This feature can be seen in the [Example](#example) above. For debugging purposes these redactions can be prevented using the `aws-smithy-http-server` feature flag, `unredacted-logging`.

Some examples of inadvertently leaking sensitive information:

- Ingesting tracing events and spans from third-party crates which do not respect sensitivity.
  - An concrete example of this would be enabling events from `hyper` or `tokio`.
- Applying middleware which ingests events including HTTP payloads or any other part of the HTTP request/response which can be bound.
