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

Additionally, constrain the `time` dependency to `<0.3.48` in `aws-smithy-types`. `time 0.3.48` introduced an E0119 coherence regression (<https://github.com/time-rs/time/issues/783>) that breaks any crate with a blanket `From` impl when `time` is in its dependency graph. Constraining `time` forces resolution to the last-good `0.3.47` across all build paths. Relax this bound once `time 0.3.49` ships.
