---
applies_to: ["server"]
authors: ["drganjoo"]
references: []
breaking: true
new_feature: true
bug_fix: false
---

With `codegen.schemaSerde` enabled, server-generated Cargo manifests record richer build metadata:

- `package.metadata.smithy.protocol` is now an array (e.g. `protocol = ["aws.protocols#restJson1"]`)
  instead of a single string, so it can list every protocol a service serves.
- A new `package.metadata.smithy.codegen-release` key records the code generator's artifact version.
  The existing `codegen-version` key (the smithy-rs source commit) is unchanged.
- Codegen settings that differ from their defaults are recorded under `package.metadata.codegen`,
  and customization settings under `package.metadata.customizationConfig`.

With `schemaSerde` disabled, server manifests retain the scalar protocol field and source commit metadata.
Client-generated manifests are unchanged.
