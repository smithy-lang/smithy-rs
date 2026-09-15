---
applies_to:
- server
authors:
- fahadzub
references: []
breaking: true
new_feature: true
bug_fix: false
---
`aws-smithy-http-server::serve` now limits concurrently accepted TCP connections to 8192 by default. Use `.max_connections(...)` to configure the limit or `.disable_connection_limit()` to restore unbounded acceptance.
