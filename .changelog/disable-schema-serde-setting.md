---
applies_to: ["client"]
authors: ["landonxjames"]
references: []
breaking: false
new_feature: true
bug_fix: false
---

Added a `disableSchemaSerde` client codegen setting that opts a single service
out of schema-based serialization/deserialization even when its protocol has
schema serde enabled, falling back to the legacy per-shape `protocol_serde`
path:

```json
{
  "plugins": {
    "rust-client-codegen": {
      "service": "com.example#MyService",
      "module": "my-service",
      "moduleVersion": "0.1.0",
      "codegen": {
        "disableSchemaSerde": true
      }
    }
  }
}
```

The setting only ever turns schema serde _off_; it cannot turn it on for a
protocol that does not have it enabled. It exists as a per-service escape hatch
during the phased rollout of schema serde, so a service that hits a schema-path
issue can be reverted without disabling its whole protocol.

Note: this setting is temporary and will be removed once schema serde is
stabilized. It is only intended as a stop-gap for users that encounter bugs with
schema serde.
