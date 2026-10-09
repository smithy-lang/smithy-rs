---
applies_to: ["server"]
authors: ["jeffzha"]
references: ["smithy-rs#0000"]
breaking: true
new_feature: true
bug_fix: false
---
Add `MetricsPool` support to `aws-smithy-http-server-metrics`. The metrics layer
now installs a request-scoped `MetricsPool` on the per-request entry and hands
out a producer handle two ways: through the request extensions
(`MetricsPoolHandle`) and through the poll-scoped `MetricsPool::current()`.
Independently-owned middleware, handlers, and libraries can contribute
heterogeneous child metrics that flatten into the single per-request entry.
`DefaultMetrics` and structs annotated with `#[smithy_metrics]` gain a flattened
pool field automatically.

This is a breaking change: `#[smithy_metrics]` now generates a `metrics_pool`
field, so existing explicit struct literals no longer compile, and `metrics_pool`
is no longer available as a user-defined field name. For example, this previously
valid literal now fails because it omits the generated field:

```rust
#[smithy_metrics]
#[metrics]
struct RequestMetrics {
    requests: u64,
}

fn new_metrics() -> RequestMetrics {
    RequestMetrics {
        requests: 1,
        default_request_metrics: None,
        default_response_metrics: None,
    }
}
```

and a struct that declares its own `metrics_pool` field now produces a duplicate
field:

```rust
#[smithy_metrics]
#[metrics]
struct RequestMetrics {
    metrics_pool: u64,
}
```

Construct these structs with `..Default::default()` (or `Default::default()`)
instead of naming every field, and rename any user-defined `metrics_pool` field.
