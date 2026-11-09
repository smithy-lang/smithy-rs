---
applies_to: ["client"]
authors: ["sfod"]
references: []
breaking: false
new_feature: true
bug_fix: false
---
`StalledStreamProtectionConfig` now accepts a `minimum_throughput_bytes_per_second` value. When set, streams that stay below this floor for longer than the grace period are aborted, catching connections that trickle data at a nonzero but persistently slow rate. Previously only fully-stalled (zero-byte) streams were detected.
