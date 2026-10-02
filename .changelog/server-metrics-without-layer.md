---
applies_to: ["server"]
authors: ["jasgin"]
references: []
breaking: false
new_feature: false
bug_fix: false
---

`aws-smithy-http-server-metrics`: extracting `Metrics<T>` in an operation handler when no `MetricsLayer` added those metrics now panics when debug assertions are enabled (e.g. in tests), with a message explaining how to add the layer. Previously this only failed the request with a 500, which is still the behavior in release builds.
