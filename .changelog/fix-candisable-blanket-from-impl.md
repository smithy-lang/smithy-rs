---
applies_to:
- client
- server
- aws-sdk-rust
authors:
- vcjana
references: []
breaking: false
new_feature: false
bug_fix: true
---

Make `CanDisable`'s `From` impl in `aws-smithy-types` concrete (`From<Duration>`) instead of a blanket `impl<T> From<T>`. The blanket impl could clash with `From` impls from other crates and break the build with `error[E0119]` on a routine dependency bump (it surfaced via `time 0.3.48`).
