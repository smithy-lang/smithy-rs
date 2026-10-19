---
applies_to: ["server"]
authors: ["jeffzha"]
references: ["smithy-rs#4887"]
breaking: true
new_feature: true
bug_fix: false
---
Add `MetricsPool` support to `aws-smithy-http-server-metrics`. The metrics layer
now stores a `SmithyMetricsPool` producer handle in the request extensions.
Independently-owned middleware and libraries that cannot name the service's
metrics entry type can pull it out and contribute heterogeneous child metrics
that flatten into the single per-request entry when the request finishes.
Structs annotated with `#[smithy_metrics]` gain the backing pool automatically
through a single hidden flattened field.

This is a breaking change on two fronts.

`#[smithy_metrics]` now injects a hidden `__smithy_metrics` field, so existing
explicit struct literals no longer compile and `__smithy_metrics` is reserved as
a field name. Construct these structs with `..Default::default()` (or
`Default::default()`) instead of naming every field:

```rust
#[smithy_metrics]
#[metrics]
struct RequestMetrics {
    requests: u64,
}

fn new_metrics() -> RequestMetrics {
    RequestMetrics {
        requests: 1,
        ..Default::default()
    }
}
```

The default-metrics entry type was also renamed `DefaultMetrics` -> `SmithyMetrics`.

