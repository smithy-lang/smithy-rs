---
applies_to: ["server"]
authors: ["jeffzha"]
references: ["smithy-rs#0000"]
breaking: false
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
