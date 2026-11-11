# RFC: Integrated standard server metrics

> Status: RFC
>
> Applies to: server

For a summarized list of proposed changes, see the [Changes Checklist](#changes-checklist) section.

## Background and overview

This RFC is a follow-on to a previous proposal, [RFC 46], which introduced the
`aws-smithy-http-server-metrics` crate as a companion to `aws-smithy-http-server`, supporting the
following two use cases:

1. Publishing a standard set of server metrics via [Metrique].
2. Providing a way for services and other middleware to publish custom metrics alongside the
   standard metrics.

This RFC proposes that `aws-smithy-http-server` should assume responsibility for collecting standard
server metrics, subsuming [`DefaultMetricsPlugin`] in `aws-smithy-http-server-metrics`. Doing so
greatly expands the kinds of standard metrics that can be collected, as observations can be made
throughout a request’s lifecycle, instead of being limited to the capabilities of HTTP plugins. A
few examples of heretofore unobservable metrics include:

- *Time spent in request deserialization.* (The innermost HTTP plugin might be able to observe when
  request deserialization is about to start, but cannot know when request deserialization is
  finished.)
- *Classification of an operation’s result as a success, [server error, or client
  error][error-trait].* (HTTP plugins cannot observe operation results before they are serialized
  into HTTP responses, and it is incorrect to infer result classification from HTTP response status
  codes.)

This RFC only covers the *collection* of standard server metrics, not their publication. Instead of
publishing metrics directly, `aws-smithy-http-server` will expose the collected metrics via an API
that middleware such as `aws-smithy-http-server-metrics` can use. This API is not tied to any
particular metrics library. Furthermore, this proposal does not preclude future integration with the
`aws-smithy-observability` crate.

> A separate companion RFC will catalogue the full list of metrics to be emitted. *This* RFC focuses
> on the general developer experience of metrics integration, where a long list of metrics would be
> distracting.

[Metrique]: https://crates.io/crates/metrique
[RFC 46]: ./rfc0046_server_request_metrics.md
[`DefaultMetricsPlugin`]:
    https://docs.rs/aws-smithy-http-server-metrics/0.4.1/aws_smithy_http_server_metrics/plugin/struct.DefaultMetricsPlugin.html
[error-trait]: https://smithy.io/2.0/spec/type-refinement-traits.html#error-trait

## Terminology

- **Metrics**: refers to “unit of work” metrics, which the Metrique documentation also calls *wide
  events*. In our case, metrics are emitted per-request.
- **Standard server metrics**: metrics that are potentially relevant to all services, rather than
  pertaining to a specific service, operation, or middleware component.
- **Middleware**: the general concept of reusable, stackable components in a Smithy-rs service.
- **Layer**: a middleware component that wraps *each route* of a Smithy service. It deals with HTTP
  requests and responses.
- **HTTP plugin**: a middleware component that wraps *each operation* in a Smithy service, but still
  deals with HTTP requests and responses. It is similar to a *layer* but is aware of which operation
  it is being applied to.
- **Upgrade plugin**: a component of Smithy-rs that converts HTTP requests into modeled operation
  inputs, and converts modeled operation outputs and errors into HTTP responses.
- **Model plugin**: a middleware component that wraps an operation in a Smithy service, dealing with
  modeled input, output, and error types.
- **Request extensions**: a feature of the [http crate] allowing divers values to be stored
  alongside a request.
- `Φ`, `μ`: these placeholders stand for a particular service and operation when discussing
  generated server SDK code. For example, `ΦServiceBuilder::μ` refers to the method used to
  configure the operation handler for a particular operation.

[http crate]: https://docs.rs/http/1.5.0/http/

## Current user experience

A Smithy-rs service owner wanting to publish standard server metrics via Metrique declares a
dependency on the `aws-smithy-http-server-metrics` crate. They must choose a version of this
dependency that indirectly depends on a version of `aws-smithy-http-server` that is compatible (in a
SemVer sense) with the version of `aws-smithy-http-server` that their generated server SDK crate
depends on. They also must declare a dependency on `metrique` so that they can configure a metrics
sink.

```toml
[dependencies]
# This version of aws-smithy-http-server-metrics depends on aws-smithy-http-server ^0.69.1, so the
# service's server SDK must use a codegen version that also generates an aws-smithy-http-server
# dependency whose version requirement overlaps with ^0.69.1.
aws-smithy-http-server-metrics = "0.4.1"
metrique = "0.1.33"
```

Then, they add [`MetricsLayer`] and [`DefaultMetricsPlugin`] to their service configuration.
(Although it is possible to use `DefaultMetricsPlugin` *without* `MetricsLayer`, doing so leads to
an incomplete set of metrics being published—notably missing a counter for in-flight requests.) This
example shows a minimal configuration:

```rust,ignore
use aws_smithy_http_server_metrics::DefaultMetricsPlugin;
use aws_smithy_http_server_metrics::MetricsLayer;
use metrique::{AttachGlobalEntrySinkExt, ServiceMetrics};
use φ_server_sdk::ΦServiceConfig;
use φ_server_sdk::server::plugin::HttpPlugins;

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

## Proposed user experience

### For services using Metrique

For services using Metrique, the experience is quite similar to the current experience described
above.

```diff
@@ -1,16 +1,14 @@
-use aws_smithy_http_server_metrics::DefaultMetricsPlugin;
 use aws_smithy_http_server_metrics::MetricsLayer;
 use metrique::{AttachGlobalEntrySinkExt, ServiceMetrics};
 use φ_server_sdk::ΦServiceConfig;
-use φ_server_sdk::server::plugin::HttpPlugins;

 // Configure Metrique's global sink.
 let metrique_stream = todo!(); // using metrique::emf::Emf, for instance
 ServiceMetrics::attach_to_stream(metrique_stream);

 // Configure the service using the generated config builder.
 let metrics_layer = MetricsLayer::new();
 let service_config_builder = ΦServiceConfig::builder()
+    .enable_standard_metrics()
     .layer(metrics_layer)
-    .http_plugin(HttpPlugins::new()
-        .push(DefaultMetricsPlugin));
+    ;
```

Note that standard metrics are *not* collected by default, as doing so would be wasteful unless the
service added middleware to consume them. The explicit call to `enable_standard_metrics()` is
necessary.

### For metrics-publishing middleware

The server will expose an API to enable middleware to access the metrics collected by the server, so
that the middleware may publish the metrics using the library of their choice.

Since the request is not yet finished while the middleware is running, middleware registers a
post-request-completion callback that the service invokes when the request is truly finished.

```rust,ignore
use aws_smithy_http_server::metrics::{RequestMetrics, RequestExt};

// In middleware's Future impl:
request.on_finish(|metrics: Option<RequestMetrics>| {
    if let Some(metrics) = metrics {
        tracing::debug!("request finished: {metrics:?}");
    }
});
```

If `ΦServiceConfigBuilder::enable_standard_metrics` was never called, the callback’s argument will
be `None`.

If `RequestExt::on_finish` is called multiple times (typically by different layers that
independently publish metrics to different destinations), the callbacks will be invoked in order of
registration.

In particular, this is the interface that `aws-smithy-http-server-metrics`’s [`MetricsLayer`] will
use to obtain the standard server metrics for injection into the user-defined per-request metrics
struct.

> *The details of the `RequestMetrics` API will be described in the companion RFC that proposes the
> complete list of standard metrics to be published.*

### The future of `DefaultMetricsPlugin`

To reiterate, under this RFC, services need to call `ΦServiceConfigBuilder::enable_standard_metrics`
instead of adding [`DefaultMetricsPlugin`] to their HTTP plugin stack. This is a breaking change to
`aws-smithy-http-server-metrics`. For services already using `DefaultMetricsPlugin`, there would be
a risk of silent loss of metrics if they upgraded their `aws-smithy-http-server-metrics` without
awareness of this change. To make it obvious that services must update their code to continue
publishing standard metrics, the breaking release of `aws-smithy-http-server-metrics` will eliminate
the `DefaultMetricsPlugin` type altogether.

## Implementation notes

> *As there will be a separate RFC proposing the complete list of standard metrics to be published,
> this section will not go into details about the specific measurements needed to populate those
> metrics.*

### Metrique-agnostic portion

Implementation is complicated by the need to maintain state throughout the entire lifecycle of each
request, beginning from entry into `ΦService` and ending only after the HTTP response is fully
sent. Two pieces of per-request state must be maintained:

1. The metrics themselves.
2. A list of callbacks (boxed `FnOnce` closures) registered with `RequestExt::on_finish`.

This state must be accessible at several points in the request lifecycle, and must accommodate a
variety of early-return paths that bypass one or more layers of the service stack, potentially
including panics, some of which are outside the control of the framework.

These constraints force a shared-state design. Per-request state is stored in an `Arc` and passed
inward via an HTTP request extension. Tower services must clone and retain this `Arc` if they
require access to the shared state after passing the HTTP request inward. For example, a Tower
service that needs to access the shared state after the inner service returns would follow this
pattern:

```rust,ignore
impl Service<http::Request<Incoming>> for MyService {
    type Response = http::Response<BoxBody>;
    type Error = Infallible;

    // fn poll_ready...

    // Pretending that the Service trait uses async fns instead of associated future types...
    async fn call(&mut self, req: http::Request<Incoming>) -> Result<Self::Response, Self::Error> {
        // MetricsState wraps an Arc for cheap cloning
        let state: Option<MetricsState> = req.extensions().get().cloned();
        let resp = self.inner.call(req).await;

        // now do something with state...

        resp
    }
}
```

Callback registration is implemented as an extension trait method `RequestExt::on_finish`.
The `RequestExt` extension trait will be implemented for all `http::Request<B>`. The implementation
boxes the callback and appends it to a `Vec` inside the shared state.

Both the list of callbacks and the metrics themselves require synchronization. A `Mutex` around the
per-request state is simple and should not pose performance issues, as no contention is expected and
the lock will be acquired a small, constant number of times.

Since `ΦService` is code-generated, we want to avoid placing server implementation details there.
Instead, the state initialization will be placed in a new non-generated `MetricsService` sandwiched
between `ΦService` and the existing `RoutingService`. `MetricsService` is also responsible for
wrapping the final `http::Response` body with a custom `http::Body` implementation that detects when
the response is fully sent (or an error occurs), finalizes the metrics, and invokes the previously
registered callbacks.

### Caveats and limitations

Smithy-rs services currently do not catch panics; uncaught panics unwind into Hyper where the
connection is dropped without sending any response. Under this proposal, `on_finish` callbacks would
*not* be invoked in case of uncaught panic. However, this proposal is compatible with panic-catching
middleware layers such as [`tower_http::catch_panic`].

Model plugins currently are severely limited in terms of what shared state they can access. Their
input type is not `http::Request`, but rather consists of the arguments to the operation
handler—something like `(Op::Input, Exts0, Exts1, ...)`, where the `ExtsN` types are chosen by the
operation handler, not the plugin. This limitation makes it impossible to capture any metrics
between the model plugins and the operation handler, as the per-request state would be inaccessible
to a Tower service interposed between them, because such a Tower service would have the same
limitations as the model plugins that surround it. Addressing this deficiency probably requires
major revisions to the API for model plugins.

Currently `ΦServiceBuilder::build` and `ΦServiceBuilder::build_unchecked` are inconsistent with
respect to where the user-specified layers are applied. `ΦServiceBuilder::build` applies the layers
to *each route*, but `ΦServiceBuilder::build_unchecked` applies them around the entire
`RoutingService`. Leaving this inconsistency in place opens the door to further ambiguity as to
where `MetricsService` should be applied in the stack. Since the inconsistency is undocumented and
there is no known reason for it, this RFC proposes to eliminate the inconsistency by changing one of
these methods to match the behavior of the other, although it is not yet decided which behavior is
desired. In any case, this will be a breaking change to the generated code, as it changes the
behavior as well as the trait bounds on one of the methods.

### Metrique integration

Metrique integration will occur primarily in `aws-smithy-http-server-metrics`, but to reduce code
duplication, we will introduce an *optional* Cargo feature `metrique` in `aws-smithy-http-server`
that, when enabled, adds a `metrique` dependency and applies `#[metrics]` to our `RequestMetrics`
struct. Then, `aws-smithy-http-server-metrics`’s `#[smithy_metrics]` proc-macro can simply inject
a field of this type into the user-defined metrics entry type, tagged with `#[metrics(flatten)]`.

[`tower_http::catch_panic`]: https://docs.rs/tower-http/0.7.1/tower_http/catch_panic/index.html

## Changes checklist

Metrique-agnostic portion, in `aws-smithy-http-server`:

- [ ] Add `ΦServiceConfigBuilder::enable_standard_metrics` method and enablement flag. Make
  enablement flag available in `ΦServiceBuilder`.
- [ ] Define the `MetricsState` struct, holding the metrics themselves and the list of registered
  callbacks, both of which must be interior-mutable.
- [ ] Define `MetricsService` and an associated future type. The implementation checks the
  enablement flag. If enabled, it initializes `MetricsState` and stores it in an HTTP request
  extension.
- [ ] Modify the code-generated `ΦService` so that the `RoutingService` is wrapped in
  `MetricsService`. Modify `ΦServiceBuilder::build` accordingly.
- [ ] Fix layer-application inconsistency between `ΦServiceBuilder::build` and
  `ΦServiceBuilder::build_unchecked`.
- [ ] Define `RequestExt::on_finish`, which accesses the `MetricsState` stored in the HTTP request
  (if present) and appends a callback to the list of registered callbacks.
- [ ] In `MetricsService`, after the inner service returns an `http::Response`, wrap the response
  body in a `MetricsFinalizingBody` and move the `MetricsState` into it. Its `http::Body`
  implementation will simply delegate to the original body. Implement `Drop` on this type, such that
  it produces the final `RequestMetrics` value and invokes each registered callback.

Metrique integration:

- [ ] In `aws-smithy-http-server`, introduce a non-default `metrique` Cargo feature and an optional
  dependency on `metrique`. When the feature is enabled, apply `#[metrics]` to `RequestMetrics`.
- [ ] In `aws-smithy-http-server-metrics`, remove `DefaultMetricsPlugin` and remove functionality
  from `MetricsLayer` that interacts with it.
- [ ] Modify `aws-smithy-http-server-metrics` so that the injected field for storage of standard
  metrics stores `RequestMetrics` instead.
