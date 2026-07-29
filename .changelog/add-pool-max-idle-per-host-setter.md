---
applies_to: ["client", "aws-sdk-rust"]
authors: ["jasgin"]
references: []
breaking: false
new_feature: true
bug_fix: false
---
Add `pool_max_idle_per_host` setter to HTTP client `Builder` and `ConnectorBuilder`,
exposing hyper's `pool_max_idle_per_host` setting to control the maximum number of idle
connections kept alive per host.
