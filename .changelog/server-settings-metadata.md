---
applies_to: ["server"]
authors: ["drganjoo"]
references: []
breaking: false
new_feature: true
bug_fix: false
---

Generated server Cargo manifests now record codegen flags that differ from their defaults
under `package.metadata.codegen`. Customization settings are recorded under
`package.metadata.customizationConfig`, omitting known built-in protocol defaults and
including the legacy CBOR routing flag's effective customization value when non-default.
Explicit false overrides are retained when the default is true or automatic.
Unset nullable settings are omitted because TOML cannot represent null. Extension
protocol settings are preserved as supplied; their runtime-defined defaults are not
known to codegen.
