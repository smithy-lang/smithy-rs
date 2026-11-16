---
applies_to: ["client", "server"]
authors: ["osrs-agent"]
references: ["smithy-rs#3765"]
breaking: false
new_feature: false
bug_fix: true
---
Generated `Debug` impls for structures and unions now redact `@sensitive` values nested in lists, sets and maps, for example appflow's `CustomAuthCredentials.credentials_map`.
