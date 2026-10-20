---
applies_to: ["server"]
authors: ["fahadzub"]
references: []
breaking: true
new_feature: true
bug_fix: false
---

Add schema-based serialization and multi-protocol routing to `aws-smithy-http-server`,
enabled with the codegen setting `schemaSerde: true`. Protocol types such as `RestJson1`
are no longer unit structs; construct them with `Default::default()`.
