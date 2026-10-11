---
applies_to: ["client"]
authors: ["landonxjames"]
references: []
breaking: false
new_feature: true
bug_fix: false
---

Added a `disableSchemaSerde` client codegen setting that opts a single service
out of schema-based serialization and deserialization even when its protocol has
schema serde enabled, falling back to the legacy per-shape `protocol_serde`
path. The setting can only turn schema serde off; it is a temporary per-service
escape hatch during the phased rollout.
