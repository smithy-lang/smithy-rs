---
applies_to: ["client", "server"]
authors: ["drganjoo"]
references: []
breaking: true
new_feature: false
bug_fix: true
---

Generated Cargo manifests now record the code generator's JAR version in
`package.metadata.smithy.codegen-version`. The Git commit previously stored there
is recorded separately in `package.metadata.smithy.codegen-version-commit`.
