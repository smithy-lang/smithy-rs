# RFC: Integrated server metrics

> Status: RFC
>
> Applies to: server

For a summarized list of proposed changes, see the [Changes Checklist](#changes-checklist) section.

This RFC proposes a built-in integration of the [Metrique] metric-publishing library into
`aws-smithy-http-server`. It builds on [RFC 46], which proposed a separate crate,
`aws-smithy-http-server-metrics`, that provides components for Metrique integration that service
owners can add to their middleware stacks. This proposal broadly addresses the same use cases as RFC
46, to wit:

1. Emitting a standard set of metrics out-of-the-box.
2. Providing a way for services and other middleware to publish custom metrics alongside the
   standard metrics.

This RFC is an enhancement of the previous proposal; the main difference is that metrics are
integrated directly into `aws-smithy-http-server` instead of being provided in a separate crate.
This approach brings the following improvements:

1. Metrics collection can be enabled by default in the framework, without requiring the service
   author to explicitly declare extra dependencies and add extra layers and plugins to their
   middleware stacks.
2. Metrics can be collected at many points during the lifecycle of handling a request, including
   some that are difficult or impossible to capture from user-defined middleware components, thus
   expanding the set of possible metrics that can be published.
3. Services would avoid a dependency-versioning issue arising from the possibility of
   `aws-smithy-http-server-metrics` depending on a *different* version of `aws-smithy-http-server`
   than the version that their generated server SDK depends on.

> A separate companion RFC will catalogue the full list of metrics to be emitted. *This* RFC focuses
on the general developer experience of metrics integration, where a long list of metrics would be
distracting.

[Metrique]: https://crates.io/crates/metrique
[RFC 46]: ./rfc0046_server_request_metrics.md

## Terminology

- **Metrics**: refers to “unit of work” metrics, which the Metrique documentation also calls *wide
  events*. In our case, metrics are emitted per-request.
- **Middleware**: the general concept of reusable, stackable components in a Smithy-rs service.
- **Layer**: a middleware component that wraps an entire Smithy service, including its routing. It
  deals with HTTP requests and responses.
- **HTTP plugin**: a middleware component that wraps *each operation* in a Smithy service, but still
  deals with HTTP requests and responses. It is similar to a *layer* but is aware of which operation
  it is being applied to.
- **Upgrade plugin**: a component of Smithy-rs that converts HTTP requests into modeled operation
  inputs, and converts modeled operation outputs and errors into HTTP responses.
- **Model plugin**: a middleware component that wraps an operation in a Smithy service, dealing with
  modeled input, output, and error types.
- `Φ`, `µ`: these placeholders stand for a particular service and operation when discussing
  generated server SDK code. For example, `ΦServiceBuilder::µ` refers to the method used to
  configure the operation handler for a particular operation.

## Current user experience

### Standard metrics

A Smithy-rs service owner wanting standard server metrics declares a dependency on the
`aws-smithy-http-server-metrics` crate. They must choose a version of this dependency that
indirectly depends on a version of `aws-smithy-http-server` that is compatible (in a SemVer sense)
with the version of `aws-smithy-http-server` that their generated server SDK crate depends on.

```toml
[dependencies]
# This version of aws-smithy-http-server-metrics depends on aws-smithy-http-server ^0.69.1, so the
# service's server SDK must use a codegen version that also generates a aws-smithy-http-server
# dependency whose version requirement overlaps with ^0.69.1.
aws-smithy-http-server-metrics = "0.4.1"
```

Then, they add [`MetricsLayer`] and [`DefaultMetricsPlugin`] to their service configuration.
(Although it is possible to use `DefaultMetricsPlugin` *without* `MetricsLayer`, doing so leads to
an incomplete set of metrics being published.) This example shows a minimal configuration:

```rust
use aws_smithy_http_server_metrics::{MetricsLayer, DefaultMetricsPlugin};
use metrique::{AttachGlobalEntrySinkExt, ServiceMetrics};
use φ_server_sdk::{ΦServiceConfig, server::plugin::HttpPlugins};

// Configure Metrique's global sink.
let metrique_stream = todo!(); // using metrique::emf::Emf, for instance
ServiceMetrics::attach_to_stream(metrique_stream);

// Configure the service using the generated config builder.
let metrics_layer = MetricsLayer::new();
let service_config_builder = ΦServiceConfig::builder()
    .layer(metrics_layer)
    .http_plugin(HttpPlugins::new()
        .push(DefaultMetricsPlugin));
```

[`MetricsLayer`]:
    https://docs.rs/aws-smithy-http-server-metrics/0.4.1/aws_smithy_http_server_metrics/layer/struct.MetricsLayer.html
[`DefaultMetricsPlugin`]:
    https://docs.rs/aws-smithy-http-server-metrics/0.4.1/aws_smithy_http_server_metrics/plugin/struct.DefaultMetricsPlugin.html

### Custom metrics

If the service wants to publish its own domain-specific metrics alongside the standard ones, they
must define their own metrics type, using [a Smithy-specific proc-macro][`smithy_metrics`] in
addition to Metrique’s [`metrics`] proc-macro:

```rust
use aws_smithy_http_server_metrics::smithy_metrics;
use metrique::unit_of_work::metrics;

#[smithy_metrics]
#[metrics]
#[derive(Default)]
struct MyMetrics {
  // The `#[smithy_metrics] proc-macro injects a hidden field into this struct to hold the
  // standard metrics.

  uri_slash_count: usize,
  is_a_teapot: bool,
}
```

They use `MetricsLayer`’s builder API to specify callbacks to populate their custom metrics. The
callbacks run at the beginning and end of each request.

```rust
use http::StatusCode;

let metrics_layer = MetricsLayer::builder()
    .init_metrics(|request| {
        let my_metrics = MyMetrics::default().append_on_drop(ServiceMetrics::sink());
        my_metrics.uri_slash_count = request.uri().path().matches('/').count();
    })
    .response_metrics(|response, metrics| {
        metrics.is_a_teapot = response.status() == StatusCode::IM_A_TEAPOT;
    })
    .build();
```

Most likely though, thit simple callback interface won’t be sufficient, because not every metric
value can be extracted from the service’s HTTP requests and responses. Realistic services will need
to emit metrics directly from their operation handlers, based on measurements taken within them. To
do so, services will need to add a field to their metrics struct, tagged with
`#[smithy_metrics(operation)]`:

```rust
use metrique::timers::Stopwatch;

#[smithy_metrics]
#[metrics]
#[derive(Default)]
struct MyMetrics {
    // as before...

    #[smithy_metrics(operation)]
    #[metrics(flatten)]
    operation_metrics: MyOperationMetrics,
}

#[metrics(subfield)]
#[derive(Default)]
struct MyOperationMetrics {
    backend_db_query_time: Stopwatch,
}
```

Then they will add an argument to their operation handler function(s), which they can use to emit
metrics during the handler’s execution.

```rust
use aws_smithy_http_server_metrics::operation::Metrics;

async fn my_operation_handler(
    input: MyOperationInput,
    metrics: Metrics<MyOperationMetrics>,
) -> MyOperationResult {
    // ...
    let stopwatch_guard = metrics.backend_db_query_time.start();
    // call database here...
    drop(stopwatch_guard);
    // ...
}
```

The type `MyOperationMetrics` in the handler argument type *must* match one of the fields in the
metric struct tagged with `#[smithy_metrics(operation)]`. Mismatches result in a failure at
*runtime, not compile time*.

[`smithy_metrics`]:
    https://docs.rs/aws-smithy-http-server-metrics/0.4.1/aws_smithy_http_server_metrics/attr.smithy_metrics.html
[`metrics`]: https://docs.rs/metrique/0.1.33/metrique/unit_of_work/attr.metrics.html

### Publishing metrics from middleware

`aws-smithy-http-server-metrics` provides [`SmithyMetricsPool`] as a way for middleware components
to publish their own per-request metrics:

```rust
// In middleware's Future impl:
if let Some(pool) = request.extensions().get::<SmithyMetricsPool>() {
    pool.with_prefix(["my_middleware"]).append(MyMiddlewareMetrics { /* ... */ });
}
```

This design crucially allows middleware to be written in a generic, reusable way, without being tied
to any particular service’s choice of metrics struct. This design also spares the service from
having to define fields in its metrics struct to accommodate middleware metrics.

[`SmithyMetricsPool`]:
    https://docs.rs/aws-smithy-http-server-metrics/0.4.1/aws_smithy_http_server_metrics/pool/struct.SmithyMetricsPool.html

## Proposed user experience

### Standard metrics

Smithy-rs services will publish standard metrics by default, as long as the service has configured
Metrique’s default sink using [`ServiceMetrics::attach_to_stream`]. Services do not need to declare
any additional dependencies or explicitly add any layers or plugins to their middleware stacks.

Services can disable publication of standard metrics, if they so choose, by calling their SDK’s
`service_config_builder.disable_standard_metrics()`. Disabling standard metrics avoids the overhead
of collecting and storing them. However, disabling standard metrics does *not* also disable custom
metrics.

[`ServiceMetrics::attach_to_stream`]:
    https://docs.rs/metrique/0.1.33/metrique/writer/trait.AttachGlobalEntrySinkExt.html#method.attach_to_stream

### Custom metrics

To publish custom metrics from an operation handler, the service defines a type to store the metrics
and adds a metrics argument to their operation handler:

```rust
use metrique::{timers::Stopwatch, unit_of_work::metrics};
use φ_server_sdk::server::Metrics;

#[metrics]
#[derive(Default)]
struct MyOperationMetrics {
    backend_db_query_time: Stopwatch,
}

async fn my_operation_handler(
    input: MyOperationInput,
    metrics: Metrics<MyOperationMetrics>,
) -> MyOperationResult {
    // ...
    let stopwatch_guard = metrics.backend_db_query_time.start();
    // call database here...
    drop(stopwatch_guard);
    // ...
}
```

This experience is quite similar to that of `aws-smithy-http-server-metrics` but with a major
difference: the service does not need to declare an outer metrics struct (like `MyMetrics` in the
previous section’s example). There is no need for the service to use a Smithy-specific proc-macro or
to configure a service-wide [`init_metrics`] callback.

The proposed `Metrics<M>` type will implement [`FromParts`], letting Smithy-rs recognize it as an
extension provider and allowing it to appear in the argument lists of operation handler functions.
The type parameter `M` may be any struct tagged with the `#[metrics]` proc-macro (or more generally,
any type that implements [`CloseEntry`]). The type `M` only needs to be specified in one place—the
argument list of the operation handler function—so there is no risk of a mismatch between multiple
places where the operation metrics type is specified, as there is with the
`aws-smithy-http-server-metrics` experience.

Each operation handler may use a metrics type tailored to a specific operation. Metrics fields that
are common to multiple operations may be shared by defining them in their own struct, then adding
that struct as a field in each operation’s metrics struct, tagged with `#[metrics(flatten)]`:

```rust
#[metrics]
struct Operation1Metrics {
    // other fields specific to operation 1...
    #[metrics(flatten)]
    common: CommonMetrics,
}
#[metrics]
struct Operation2Metrics {
    // other fields specific to operation 2...
    #[metrics(flatten)]
    common: CommonMetrics,
}

#[metrics(subfield)]
struct CommonMetrics {
    splines_reticulated: u32,
}
```

[`init_metrics`]:
    https://docs.rs/aws-smithy-http-server-metrics/0.4.1/aws_smithy_http_server_metrics/layer/builder/struct.MetricsLayerBuilder.html#method.init_metrics
[`FromParts`]:
    https://docs.rs/aws-smithy-http-server/0.69.1/aws_smithy_http_server/request/trait.FromParts.html
[`CloseEntry`]: https://docs.rs/metrique-core/0.1.24/metrique_core/trait.CloseEntry.html

### Publishing metrics from middleware

The new experience will be *conceptually* similar to the experience from
`aws-smithy-http-server-metrics`, but with the implementation details further abstracted.

```rust
use aws_smithy_http_server::metrics::RequestExt; // extension trait providing `metrics<M>()`

#[metrics]
#[derive(Default)]
struct MyMiddlewareMetrics {
    power: f32,
}

// In middleware's Future impl:
let metrics /* : MyMiddlewareMetricsGuard<_> */ = request.metrics::<MyMiddlewareMetrics>();
metrics.power = 1.21e9;
```

### The future of `aws-smithy-http-server-metrics`

All use cases served by `aws-smithy-http-server-metrics` will now be covered by
`aws-smithy-http-server` itself, so the `aws-smithy-http-server-metrics` crate can be deprecated.

Enabling metrics by default should be considered a breaking change to `aws-smithy-http-server`,
because otherwise services already publishing metrics by other means would suddenly start publishing
a redundant set of metrics when they upgraded their dependencies to the latest SemVer-compatible
versions. Releasing a breaking change to `aws-smithy-http-server` (with a SemVer bump) means that
the current version of `aws-smithy-http-server-metrics` will no longer be compatible with it.

## Implementation plan

> *As there will be a separate RFC proposing the complete list of standard metrics to be published,
this section will not go into details about the specific measurements needed to populate those
metrics.*

Internally, `aws-smithy-http-server` will define a metrics struct with two fields: one to hold
standard server metrics, and another to hold a [`MetricsPool`] that stores metrics from middleware
and the operation handler. The field that holds standard server metrics will use `Option`, and its
value will remain `None` when standard metrics have been disabled.

The generated `ΦServiceBuilder::build` method will wrap the entire [`RoutingService`] with a new
layer that initializes the metrics struct, calls `append_on_drop` on it, and stores it in a
request extension. This layer also has the opportunity to collect any necessary measurements before
and after delegating to the inner layer.

Each generated `ΦServiceBuilder::µ` and `ΦServiceBuilder::µ_service` method will be modified to wrap
the per-operation Tower service with a new HTTP plugin that accesses the metrics from the request
extension where it was previously stored and has the opportunity to update it with any per-operation
metrics. The operation handler itself will also be wrapped with a Tower service that has the
opportunity to take any measurements that must surround only the handler itself, such as a timer for
handler execution time.

The [`Upgrade`] service, which converts between HTTP requests/responses and Smithy data types, will
need to be modified to classify the operation’s result (success, server error, or client error) and
store that classification in a response extension, so that metrics can make use of it. It is
important that this classification be based on Smithy model traits like [@error] rather than
guessing based on HTTP status codes.

Some metrics, such as an in-flight requests counter, may require shared state. The generated
`MakeService` will be responsible for initializing such state and storing it in an `Arc` so that it
can be shared across all cloned services.

To allow publishing metrics from operation handlers, `aws-smithy-http-server` will define a
`Metrics<M>` extension-provider type that implements `FromParts` so that it can appear in an
operation handler’s argument list. The `FromParts` implementation initializes a metrics entry of
type `M`, appends it to the `MetricsPool` from the metrics struct stored in the request extensions,
and exposes its `AppendAndCloseOnDrop` handle to the operation handler.

To allow publishing metrics from middleware, `aws-smithy-http-server` will define an extension trait
`RequestExt` that adds a `metrics<M>` method to `http::Request`. This method behaves similar to
`<Metrics<M> as FromParts>::from_parts`: it initializes a metrics entry of type `M`, appends it to
the `MetricsPool` from the metrics struct stored in the request extensions, and returns its
`AppendAndCloseOnDrop` handle.

[`MetricsPool`]: https://docs.rs/metrique-util/latest/metrique_util/struct.MetricsPool.html
[`SlotGuard`]: https://docs.rs/metrique/0.1.33/metrique/slot/struct.SlotGuard.html
[`RoutingService`]:
    https://docs.rs/aws-smithy-http-server/0.69.1/aws_smithy_http_server/routing/struct.RoutingService.html
[`Service`]: https://docs.rs/tower-service/0.3.3/tower_service/trait.Service.html
[`Upgrade`]:
    https://docs.rs/aws-smithy-http-server/0.69.1/aws_smithy_http_server/operation/struct.Upgrade.html
[@error]: https://smithy.io/2.0/spec/type-refinement-traits.html#error-trait

## Changes checklist

- [ ] Add `ΦServiceBuilder::disable_standard_metrics` method.
- [ ] Define the top-level metrics struct and outermost layer to initialize it. Modify
  `ΦServiceBuilder::build` to wrap entire service with this outermost layer.
- [ ] Define outermost HTTP plugin, and modify `ΦServiceBuilder::µ` and `ΦServiceBuilder::µ_service`
  to wrap each operation’s Tower service with it.
- [ ] Define innermost model plugin, and modify `ΦServiceBuilder::µ` and
  `ΦServiceBuilder::µ_service` to wrap each operation handler with it.
- [ ] Add operation result classification to `Upgrade` service.
- [ ] Add `Metrics<M>` extension to allow operation handlers to emit metrics.
- [ ] Add `RequestExt::metrics<M>` extension trait method to allow middleware to emit metrics.
- [ ] Publish a final version of `aws-smithy-http-server-metrics` solely to update the README on
  [crates.io] with information about the crate’s deprecation.

[crates.io]: https://crates.io/
