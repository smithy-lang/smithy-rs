---
applies_to: ["server"]
authors: ["drganjoo"]
references: []
breaking: true
new_feature: true
bug_fix: false
---

- Setting `schemaSerde: true` generates a server SDK that supports multiple protocols.
- `package.metadata.smithy.protocol` is now an array (e.g. `protocol = ["aws.protocols#restJson1"]`).
- A new `package.metadata.smithy.codegen-release` key records the code generator's artifact version.
  The existing `codegen-version` key (the smithy-rs source commit) is unchanged.
- Codegen settings that differ from their defaults are recorded under `package.metadata.codegen`,
  and customization settings under `package.metadata.customizationConfig`.
