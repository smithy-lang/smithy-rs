<!-- Give your RFC a descriptive name saying what it would accomplish or what feature it defines -->
RFC: Client request metrics
===========================

<!-- RFCs start with the "RFC" status and are then either "Implemented" or "Rejected".  -->
> Status: RFC
>
> Applies to: client

<!-- A great RFC will include a list of changes at the bottom so that the implementor can be sure they haven't missed anything -->
For a summarized list of proposed changes, see the [Changes Checklist](#changes-checklist) section.

<!-- Insert a short paragraph explaining, at a high level, what this RFC is for -->
This RFC adds `aws-smithy-client-metrics`, a crate that records per-request metrics for smithy-rs clients and the AWS SDK and emits them through [metrique]. Installing one interceptor on a client records every SDK call's duration, attempts, retries, final status, throttles and connection reuse, then applies configurable destination and missing-pool policies. It is the client-side counterpart to `aws-smithy-http-server-metrics` ([RFC-0046](./rfc0046_server_request_metrics.md)).

This is the first step toward per-request ("unit of work") data from smithy-rs clients. Later steps, such as checksum validation outcomes and retry decisions, need producers inside the SDK and are out of scope here. See [Future work](#future-work).

<!-- The "Terminology" section is optional but is really useful for defining the technical terms you're using in the RFC -->
Terminology
-----------

- **metrique**: A library for unit-of-work ("wide event") metrics. <https://crates.io/crates/metrique>
- **SDK call**: One `send()` on a fluent builder, including all of its retry attempts.
- **Application request**: The unit of work the caller is handling, such as one request to the caller's service. It can make many SDK calls.
- **`MetricsPool`**: A `metrique-util` type that lets code that can't name the parent entry type contribute fields to it. `with_metrics_pool(handle, fut)` makes a pool available through `MetricsPool::current()` while `fut` is polled.
- **Standalone entry**: A metrique entry written for a single SDK call, not attached to an application request.

<!-- Explain how users will use this new feature and, if necessary, how this compares to the current user experience -->
The user experience if this RFC is implemented
----------------------------------------------

### In the current version of the SDK

Teams that want per-request metrics write their own interceptors, usually a few hundred lines, and install it on each client with `.interceptor(..)`. The interceptor tracks start times and attempt counts in the `ConfigBag`, reads the service and operation from `Metadata`, and builds its own entry types. See [Appendix A](#appendix-a-what-users-write-today) for an outline.

Where the data goes varies:

- **Most commonly, one standalone entry per SDK call**, sometimes also one per attempt, written to a global sink such as `ServiceMetrics`. These entries aren't connected to the application request that made the call. Relating them requires adding a request ID to both entries and joining them later.
- **Less often, into the application request's entry.** To avoid changing call sites, these interceptors use their own task-local scope: middleware installs a per-request accumulator, and the interceptor writes into it. This is the problem `MetricsPool` in `metrique-util` now solves generically, but these implementations predate it and are tied to one team's entry type.
- **Sometimes not to metrique at all**, but to another metrics library or to structured logs.

This has four problems:

- **The same collection logic is rewritten in every service.** Each implementation re-derives attempt counts, success, client-error and server-error outcomes from status codes, and per-call versus per-attempt timing.
- **Calls that time out are easy to lose.** The orchestrator enforces timeouts by dropping futures, so an attempt timeout skips `read_after_attempt` and an operation timeout skips `read_after_execution` ([smithy-rs#4318]), even though both hooks are documented to always run. An interceptor that emits from those hooks silently records nothing for the calls most worth seeing. Some implementations add their own workaround; many don't.
- **Data is missing or approximated.** Detecting throttling properly means re-running the client's retry classifiers. Most implementations instead check a hand-maintained list of status or error codes, or skip it. New instrumentation requires additional work to integrate. For example, connection reuse requires attaching the recently added `CaptureHttpAttemptTelemetry`.
- **Results aren't comparable.** Field names and semantics differ between implementations, and none is checked against the orchestrator's lifecycle, so the same metric can mean different things in different services.

### Once this RFC is implemented

Install `ClientMetrics` once per client:

```rust,ignore
use aws_smithy_client_metrics::ClientMetrics;

let s3 = aws_sdk_s3::Client::from_conf(
    aws_sdk_s3::config::Builder::from(&sdk_config)
        .interceptor(ClientMetrics::new())
        .build(),
);
```

To configure it, construct it with the builder instead:

```rust,ignore
let metrics = ClientMetrics::builder()
    .qualifier("Audit")                 // distinguish this S3 client
    .standalone_sink(audit_sink)        // override `ServiceMetrics` for standalone entries
    .build();

let audit_s3 = aws_sdk_s3::Client::from_conf(
    aws_sdk_s3::config::Builder::from(&sdk_config)
        .interceptor(metrics)
        .build(),
);
```

The builder also configures where each call's fields go and what happens when a current pool is unavailable; see [Destinations](#destinations).

Teams that don't use `MetricsPool`-like implementations for a single entry per application request can select `Destination::Standalone` with `.standalone()`. Each SDK call then writes a standalone entry to the global `ServiceMetrics` sink, or to the sink set with `.standalone_sink(..)`, which matches what their hand-written interceptors produce today, with the fields below.

To put SDK fields into the application request's entry instead, run request handling inside the request's pool, once per application request. This usually lives in middleware:

```rust,ignore
#[metrics(rename_all = "PascalCase")]
struct RequestMetrics {
    route: &'static str,
    #[metrics(flatten)]
    sdk: MetricsPool,
}

let request = RequestMetrics { route: "/thumbnail", sdk: MetricsPool::new() }
    .append_on_drop(ServiceMetrics::sink());

with_metrics_pool(request.sdk.handle(), async {
    let item = ddb.get_item()/* .. */.send().await?;   // SDK call sites are unchanged
    let obj = s3.get_object()/* .. */.send().await?;
    render(item, obj).await
})
.await
```

When `request` drops, one entry is emitted for the application request:

```
Route                                /thumbnail
SdkDynamoDBGetItemInvocationDuration 4.1 ms
SdkDynamoDBGetItemRetryCount         0
SdkDynamoDBGetItemSuccess            1
SdkS3GetObjectInvocationDuration     38.7 ms
SdkS3GetObjectAttempts               2
SdkS3GetObjectRetryCount             1
SdkS3GetObjectThrottles              1
SdkS3GetObjectConnectionReused       0
SdkS3GetObjectHttpStatusCode         200
...
```

The customer configures no service or operation names. They come from the operation metadata that generated clients already store.

#### Fields

##### Field names

In a pool, each field name starts with `Sdk{Service}{Qualifier}{Operation}`. For example, an S3 `GetObject` retry count is `SdkS3GetObjectRetryCount`. Including the service and operation prevents calls to different services or operations from overwriting each other. Calls with the same service, qualifier and operation still collide; the pool keeps the last call. Whether repeated calls should instead be aggregated or indexed remains an open question.

Use `.qualifier("Audit")` when one request may call the same operation through multiple clients for a service. It inserts the qualifier between the service and operation, producing `SdkS3AuditGetObjectRetryCount`. Standalone entries record it as `Qualifier = "Audit"`.

Use `.prefix("AuditLog")` to replace `Sdk{Service}{Qualifier}` in pooled field names, producing `AuditLogGetObjectRetryCount`. The operation remains part of the name. A prefix does not affect standalone entries; a qualifier still appears in their `Qualifier` field.

##### Recorded fields

| Field | Meaning |
|---|---|
| `InvocationDuration` | Time from immediately before request serialization until execution completes, across all attempts. Excludes earlier interceptor work and reading a streaming response body. Omitted if timing never starts or the time source moves backwards. |
| `Attempts` / `RetryCount` | Number of attempts, and attempts minus one |
| `HttpStatusCode` | HTTP status of the final attempt, when a response was received |
| `Success` | The final attempt produced a successful output |
| `ClientError` / `ServerError` | The final status was 4xx / 5xx |
| `Throttles` | Attempts that the client's retry classifiers marked as throttling. This interceptor reruns the classifiers after each attempt. |
| `ErrorType` | Why a failed call failed: `Throttling`, `ServiceError` (any non-throttling error response), `ConfiguredTimeout` (operation, attempt, connect or read timeout), `IO` or `Other`. Omitted on success and on cancellation. |
| `RetryDelay` | Elapsed time between attempts. Approximates retry backoff and rate-limiter waits, and can include interceptor or orchestrator overhead. |
| `AttemptTimeouts` | Attempts cut short by the attempt timeout |
| `OperationTimeout` | The operation timeout fired before the call completed |
| `Cancelled` | The caller dropped the `send()` future before the call completed |
| `ConnectionAcquireDuration` | The final attempt's time to get a connection from the HTTP client, when it reports it |
| `ConnectionReused` | The final attempt used an already-used connection, when the HTTP client reports it |
| `ConnectorCallDuration` | The final attempt's connector time up to the response head, when the HTTP client reports it |

Standalone entries carry the same fields without a prefix, plus `SdkService`, `Qualifier`, `Operation` and a timestamp. Standalone entries written because an expected pool was missing also carry `MetricsPoolFallback = 1`.

#### Timeouts and cancellation

Timeouts and cancellation do not bypass the configured routing policy. Each call produces one completed record, including calls that time out. When present, `InvocationDuration` covers the interval from immediately before serialization until the call completes, times out or is dropped.

| How the call ends | What is recorded |
|---|---|
| An attempt times out and a retry succeeds | `AttemptTimeouts = 1`, `Success = 1`, plus the retried attempt as usual |
| The last attempt times out | `AttemptTimeouts` counts it, `Success = 0` |
| The operation timeout fires | `OperationTimeout = 1`, `Success = 0` |
| The caller drops the `send()` future | `Cancelled = 1`, `Success = 0` |

#### Destinations

Destination selection and missing-pool handling are configured separately:

```rust,ignore
use aws_smithy_client_metrics::{ClientMetrics, Destination, MissingPoolBehavior};

ClientMetrics::builder()
    .qualifier("Audit")
    .prefix("AuditLog")
    .destination(Destination::CurrentPool)
    .on_missing_pool(MissingPoolBehavior::EmitStandalone)
    .standalone_sink(sink)
    .build()
```

| Destination | Behavior |
|---|---|
| `Destination::CurrentPool` | Use the pool in scope when `send()` starts. Apply the configured missing-pool behavior when none is in scope. |
| `Destination::Pool(handle)` | Always use the given pool. This suits a client that lives as long as the pool. |
| `Destination::Standalone` | Route every call to a standalone entry. This suits batch jobs, CLIs and workers without request entries; emission still requires a configured or global sink. |

When `Destination::CurrentPool` finds no pool, it always logs a rate-limited warning. The missing-pool behavior controls what happens to the metrics record:

| Missing-pool behavior | Result |
|---|---|
| `EmitStandalone` | Emit a standalone entry with `MetricsPoolFallback = 1`. This is the default. |
| `Drop` | Drop the metrics entry without affecting the SDK call. |

`ClientMetrics::new()` selects `CurrentPool` with `EmitStandalone`. The builder retains convenience methods: `require_current_pool()` selects `CurrentPool` with `Drop`, `metrics_pool(handle)` selects `Pool(handle)`, and `standalone()` selects `Standalone`.

Standalone entries go to `.standalone_sink(..)`, or to metrique's global `ServiceMetrics` sink when none is set. The sink is used only by `Destination::Standalone` and missing-pool behaviors that emit standalone entries. If neither sink is available, the entry is dropped with a rate-limited warning.

`MetricsPoolFallback = 1` means the current pool was expected but not found. The field is present only on standalone entries emitted by a missing-pool behavior.

To provide a current pool for one call, scope that call directly:

```rust,ignore
with_metrics_pool(
    request.sdk.handle(),
    s3.get_object()/* .. */.send(),
)
.await?;
```

This applies to clients using `Destination::CurrentPool`. Clients configured with an explicit pool or standalone destination keep that configuration.

#### Installing on a single call

`ClientMetrics` can also be added to one call instead of the whole client:

```rust,ignore
s3.get_object()/* .. */
    .customize()
    .interceptor(ClientMetrics::builder().qualifier("Audit").build())
    .send()
    .await?;
```

That call is recorded exactly as with a client-level install, with the same fields, routing configuration and timeout handling. Other calls aren't recorded. This suits teams that only want metrics for some calls, or one call that needs its own qualifier or destination.

Install it at one level only. If both the client and the call have a `ClientMetrics`, the call is recorded once, by whichever instance runs first, and the other logs a rate-limited warning.

#### Installing as a runtime plugin

`ClientMetrics` also implements `RuntimePlugin`. The AWS SDK generates `runtime_plugin(..)` as crate-private (`enableUserConfigurableRuntimePlugins: false`), so SDK users install it with `.interceptor(..)`. Other generated clients that expose `config::Builder::runtime_plugin` can use either method; both behave the same. Generated clients don't expose `runtime_plugin(..)` on `.customize()`, so a single call always uses `.interceptor(..)`.

<!-- Explain the implementation of this new feature -->
How to actually implement this RFC
----------------------------------

### Crate

A new 0.x crate, `rust-runtime/aws-smithy-client-metrics`, registered in `CrateSet.SMITHY_RUNTIME_COMMON`. It depends on `aws-smithy-async`, `aws-smithy-runtime`, `aws-smithy-runtime-api`, `aws-smithy-types`, `metrique`, `metrique-util` (feature `metrics-pool`) and `tracing`.

It changes no existing crate, generated code or public SDK API. It works with generated clients that expose interceptor configuration and requires no generated-code changes. Adding it may update compatible 1.x runtime dependencies to versions containing the APIs used by this crate.

```
src/
  lib.rs      ClientMetrics, builder, routing types, RuntimePlugin impl
  collect.rs  Intercept impls; per-call state
  record.rs   RequestRecord: plain Rust, no metrique types
  emit.rs     RequestRecord → metrique entries
```

Collection (`collect.rs`, `record.rs`) is kept separate from emission (`emit.rs`), and `RequestRecord` contains no metrique types. This keeps the current collector independent of metrique and allows it to be replaced by or adapted to backend-neutral capture types later.

### Collection

All state for one call lives in an `InvocationState` stored in the `ConfigBag`'s interceptor-state layer. That layer persists across retries; request extensions are reset on each attempt.

| Hook | Work |
|---|---|
| `read_before_execution` | Create `InvocationState` and capture the current or configured destination pool. Client-level interceptors run before operation metadata is applied. |
| `read_before_serialization` | Read the service and operation from `Metadata`, record the start time, keep the time source and read the configured timeouts. This is the first hook after all plugin configuration is applied that has `RuntimeComponents`. |
| `read_before_attempt` | Mark the previous attempt as timed out if it never reached `read_after_attempt`, then start an `AttemptRecord` |
| `modify_before_transmit` | Attach a `CaptureHttpAttemptTelemetry` to the request, reusing an existing one, and keep a handle to it |
| `read_after_attempt` | Record status, success, error type, throttling (`run_classifiers_on_ctx` with the client's classifiers, `ErrorKind::ThrottlingError`) and connection telemetry |
| `read_after_execution` | Classify the error if the call failed before any attempt, finish the record and apply its routing policy |
| `Drop` of `InvocationState` | If the record was not completed yet, finish it as an operation timeout or a cancellation and apply its routing policy |

Details:

- **The pool is captured once, at the start of the call.** All attempts and the final emission use the same destination, even if the poll context changes during retries.
- **Missing-pool behavior is separate from destination selection.** It applies only to `Destination::CurrentPool`; explicit pools and standalone emission do not consult it. A dropped metrics entry never fails the SDK call.
- **Time comes from the client's `TimeSource`.** This follows the repo's ban on `std::time` and keeps durations testable.
- **Names are normalized and interned.** Generated clients store the Smithy `sdkId` (for example `"CloudWatch Logs"`). Names are reduced to ASCII alphanumerics and leaked once each, because metrique pool prefixes must be `&'static str`. The set of names is bounded by the services and operations a process calls.
- **Each call is recorded once.** `InvocationState` records which `ClientMetrics` instance owns the call. If a second instance is installed on the same call, for example one on the client and one on the operation, it does nothing and logs a rate-limited warning.
- **Timeouts never bypass routing.** The orchestrator runs each attempt, including `read_after_attempt`, inside the attempt timeout, and the whole operation, including `read_after_execution`, inside the operation timeout. When either fires, those hooks are skipped:
  - An attempt with no `read_after_attempt` is marked as timed out when the next attempt starts, or when the call finishes. If the call is dropped instead, for example because the operation timeout fires during the backoff after an attempt timeout, the attempt is marked as timed out when more than the attempt timeout has passed since it started, since an attempt can't run longer than that. Otherwise it was still in flight. In each case a timed-out attempt's duration is the configured attempt timeout. Whatever the HTTP client recorded about its connection before it stalled is kept, so a call that ends in a timeout still reports `ConnectionReused`.
  - `InvocationState` completes and routes the record from its `Drop` impl if `read_after_execution` never ran. The `ConfigBag` that owns it is dropped as soon as the operation timeout returns its error, or when the caller drops the future, so an emitted entry is written promptly and before the request's pool closes. The interceptor-state layer is never cloned, and a `finished` flag guards the normal path, so each call is routed once.
  - The orchestrator doesn't say why the future was dropped. The record is reported as `OperationTimeout` when a configured operation timeout had elapsed (and a sleep implementation was set, so it could fire), and as `Cancelled` otherwise.

### Emission

- **Pooled:** `handle.with_prefix(["sdk", service, qualifier?, operation]).append(InvocationMetrics)`, with `[prefix, operation]` as the segments when a prefix is set. The prefix follows the parent's name style, so a `PascalCase` parent gets `SdkS3GetObjectRetryCount`. The operation is part of the prefix because `MetricsPool` resolves key collisions by keeping the later child and dropping the earlier one entirely. Without the operation, `GetItem` and `PutItem` in one request would erase each other. Repeated calls to the same operation still collide, and the last call wins. A default pool retains 128 children and evicts the oldest when full. It also discards appends after the parent closes. Both conditions emit rate-limited warnings. Capacity is configurable, and callers must keep the parent open until calls finish.
- **Standalone:** one `StandaloneInvocationMetrics` entry per SDK call, with a timestamp from the client's time source, written through `append_and_close` to the configured sink or `ServiceMetrics::try_sink()`.

This is a workaround for [smithy-rs#4318] inside this crate. The proper orchestrator fix is to run `finally_attempt` and `finally_op` after a timeout and record the timeout source in the context. When that support is available, this crate must read the explicit attempt or operation timeout from the context before removing its elapsed-time heuristic; the current hooks do not infer those flags from an error alone. The workaround remains until the crate's minimum supported `aws-smithy-runtime` includes both the orchestrator support and this integration. The `Drop` path remains for caller cancellation, which no orchestrator change can cover.

### Testing

Integration tests drive the real orchestrator through `aws_smithy_runtime::client::orchestrator::operation::Operation`, using an HTTP client that reports connection telemetry. They cover:

- a generated S3 client installation that emits pooled fields through the public `.interceptor(..)` path;
- the pooled and standalone key sets, pinned exactly, because these names are the public contract;
- retries and throttles across attempts, failures, distinct-operation isolation, repeated-operation last-call behavior, name normalization, qualifiers and prefix overrides;
- every destination and missing-pool behavior, protection against double-installation, and installation as a runtime plugin;
- durations measured on a `ManualTimeSource`;
- attempt timeouts with and without a retry, an attempt timeout followed by an operation timeout during the backoff, operation timeouts in pooled and standalone modes, caller cancellation, that completed calls aren't emitted a second time from `Drop`, and that connection telemetry survives a timed-out attempt.

### Alternatives considered

| Alternative | Why not |
|---|---|
| Generate an `.instrument(sink)` method on `CustomizableOperation` with a typed capture handle | Puts `aws-smithy-observability` types in the public SDK API, which requires promoting that crate to 1.x first. The interceptor provides this prototype's metrique output without changing generated APIs; typed capture remains separate future work. |
| Put the record type in `aws-smithy-observability` now | No consumer outside metrique needs it yet. Keeping it private avoids freezing a shape before the in-SDK producers exist. |
| Make metrique a dependency of the runtime crates | Adds a 0.x dependency to 1.x crates and makes users who don't use metrique pay for it. |
| Combine destination and fallback into `Current`, `RequireCurrent`, `Explicit` and `Standalone` modes | A mode conflates where metrics should go with what to do when the current pool is missing. Separate settings make fallback explicit and let `require_current_pool()` mean warn and drop the metrics entry. |
| Add a per-call pool override interceptor | `with_metrics_pool(handle, call)` already supplies a pool to one call using `Destination::CurrentPool`. Overriding an explicit pool or standalone destination would support uncommon mixed-routing cases while adding public API and ConfigBag coordination. |
| Emit standalone entries per attempt by default | Doubles the entry volume, and most callers want one record per call. Left as a possible opt-in. |
| Name the crate `aws-smithy-runtime-metrics` or `aws-smithy-http-client-metrics` | Users rarely see `aws-smithy-runtime` directly, and `aws-smithy-http-client` is the connector layer, which provides only connection data. `client-metrics` describes what users get and mirrors `aws-smithy-http-server-metrics`. |
| Name the crate `aws-smithy-observability-metrique` | That name would imply a Metrique-backed implementation of generic APIs in `aws-smithy-observability`, parallel to `aws-smithy-observability-otel`. This crate currently provides client-specific instrumentation and writes directly to Metrique. The name may fit a future adapter if `aws-smithy-observability` gains a backend-neutral unit-of-work API. |

### Future work

Near term, preserve the current `ClientMetrics` installation while SDK-owned validation producers report through backend-neutral captures that this crate temporarily converts and routes to metrique. Long term, move generic capture-to-metrique conversion into `aws-smithy-observability-metrique`; `aws-smithy-client-metrics` then either remains a thin client-wide wrapper or is replaced. Request metrics and streaming validation keep their separate completion lifecycles throughout.

#### Direction

Today, `aws-smithy-client-metrics` collects client lifecycle data, converts it to metrique fields, and routes it to a pool or standalone entry.

As an intermediate step, extend `ClientMetrics` to install and consume backend-neutral captures defined in `aws-smithy-observability` and populated by SDK-owned checksum and content-length producers. `ClientMetrics` temporarily retains metrique conversion and routing. This exposes validation without changing existing metrics call sites or adding metrique dependencies to SDK producers.

The long-term layering is:

```text
Data:     SDK producers → aws-smithy-observability → aws-smithy-observability-metrique → metrique
Customer: customer → aws-smithy-client-metrics (optional wrapper) → aws-smithy-observability-metrique
```

`aws-smithy-observability-metrique` remains independent of `aws-smithy-client-metrics`. If the client-specific crate remains, it delegates conversion to the generic adapter and may re-export relevant types. Otherwise, the generic adapter replaces it. SDK-owned producers become the sole source of captured data in either case.

Existing `ClientMetrics` customers should not need to change SDK call sites. The integration observes validation already performed by the SDK; it does not enable validation. Request metrics complete when `send()` returns, while streaming validation settles when the body ends. Correctness-sensitive consumers use the typed capture API directly rather than reading metrique output. The integration must preserve prompt request metrics; otherwise validation emission should initially require explicit enablement.

#### Plan

1. Validate the current client-wide interceptor and `RuntimePlugin` experience, especially ambient pool attachment and missing-scope behavior.
2. Define backend-neutral capture types and lifecycle semantics in `aws-smithy-observability`; producers do no work without an installed capture. The capture mechanism must let a reusable client interceptor create or join one capture per invocation, and let client-wide telemetry coexist with an explicit typed capture on the same call regardless of interceptor order.
3. Add checksum and content-length producers, then extend `ClientMetrics` to install and consume their backend-neutral captures.
4. Add producers for attempts, retry decisions, backoff, stop reasons, and connection details, removing corresponding collection from this crate as each becomes available.
5. Add `aws-smithy-observability-metrique`, move generic conversion into it, and decide whether `aws-smithy-client-metrics` remains a wrapper or is replaced.
6. Define how body-settled validation attaches to metrique entries without delaying request metrics or requiring the parent entry to wait.
7. Fix [smithy-rs#4318] and update this crate to read explicit attempt and operation timeout sources from the context before removing its heuristics. Keep AWS-specific fields in an AWS-layer producer, and later consider sharing producer data with OpenTelemetry and server request metrics.

### Open questions

1. Should repeated calls to the same operation in one application request be distinguishable, for example with a call index or summed counters, instead of the last call winning?
2. Should per-attempt standalone entries be offered as an opt-in?
3. Is the global `ServiceMetrics` sink the right default for standalone entries, or should an unconfigured client write no standalone entries at all?
4. What should the emitted field names be, and should they follow an existing convention? The names are the crate's public contract, so this should be settled before the first release. Current names follow `aws-smithy-http-server-metrics` where the two overlap (`HttpStatusCode`, `ClientError`, `ServerError`) and are otherwise new (`InvocationDuration`, `Attempts`, `Throttles`). Conventions to consider:
   - **The AWS SDK for Java 2.x metrics** (for example `ApiCallDuration`, `ApiCallSuccessful`, `RetryCount`, `ServiceId`, `OperationName`), which would make dashboards consistent across SDK languages.
   - **The SDK's existing OpenTelemetry metrics** (for example `smithy.client.call.duration` and `smithy.client.call.attempt.duration`), so the per-request and aggregate views use the same vocabulary.
5. Is `Sdk{Service}{Qualifier}{Operation}{Field}` the right default naming (for example `SdkS3AuditGetObjectRetryCount`) for field names SDK calls add to the application request's entry? It ensures no collisions and the `prefix` builder method allows for override.

<!-- Include a checklist of all the things that need to happen for this RFC's implementation to be considered complete -->
Changes checklist
-----------------

- [x] Create the `aws-smithy-client-metrics` crate with `ClientMetrics` and `ClientMetricsBuilder`
- [x] Implement `Intercept` and `RuntimePlugin` for `ClientMetrics`
- [x] Pooled and standalone emission, destination selection and missing-pool behavior
- [x] Integration tests against the orchestrator, including pinned key names
- [x] Complete and route records for calls that end in an attempt timeout, an operation timeout or cancellation
- [x] Register the crate in `rust-runtime/Cargo.toml`, `CrateSet.kt` and the sdk-lints README allowlist
- [x] Add `external-types.toml`, the README and a changelog entry
- [ ] Resolve repeated-call semantics, field names and the standalone sink default before the first release
- [x] Regenerate `aws/sdk/Cargo.lock` to include the crate's dependencies
- [x] Add a generated AWS SDK integration test for the crate-doc installation path

Appendix A: What users write today
----------------------------------

A condensed outline of a typical hand-written interceptor. It is installed on the client and writes one standalone entry per SDK call to the global sink.

```rust,ignore
#[derive(Debug, Clone)] struct InvocationStart(Instant);
impl Storable for InvocationStart { type Storer = StoreReplace<Self>; }
#[derive(Debug, Clone)] struct AttemptStart(Instant);
impl Storable for AttemptStart { type Storer = StoreReplace<Self>; }

#[metrics(rename_all = "PascalCase")]
struct SdkInvocation {
    #[metrics(timestamp)]
    timestamp: SystemTime,
    service: &'static str,
    operation: String,
    status_code: Option<String>,
    success: bool,
    error: bool,
    fault: bool,
    throttled: bool,
    retry_count: u32,
    #[metrics(unit = Millisecond)]
    latency: Option<Duration>,
}

impl Intercept for SdkMetricsInterceptor {
    fn read_before_execution(&self, _: &BeforeSerializationInterceptorContextRef<'_>, cfg: &mut ConfigBag)
        -> Result<(), BoxError> {
        // store the start time
    }
    fn read_before_attempt(/* .. */) -> Result<(), BoxError> {
        // store the attempt start
    }
    fn read_after_attempt(/* .. */) -> Result<(), BoxError> {
        // record status and outcome; optionally write a per-attempt entry
        // throttled: usually a hard-coded list of status or error codes
    }
    fn read_after_execution(/* .. */) -> Result<(), BoxError> {
        // read the attempt count, build SdkInvocation, append to ServiceMetrics
        // (skipped entirely when the operation timeout fires)
    }
}
```

[metrique]: https://crates.io/crates/metrique
[smithy-rs#4318]: https://github.com/smithy-lang/smithy-rs/issues/4318
