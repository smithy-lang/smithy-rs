---
applies_to:
- server
authors:
- amodam-user
references:
- smithy-rs#4669
breaking: false
new_feature: true
bug_fix: false
---

Server-generated Rust enums for named Smithy `@enum` shapes now additionally derive `Copy`. Server enums are closed (no `Unknown(...)` fallback) and contain only unit variants, so they are universally `Copy`-eligible. Unnamed `@enum` string shapes are unaffected because they generate a `String` newtype that cannot be `Copy`.
