---
applies_to:
- client
- server
- aws-sdk-rust
authors:
- lauzadis
references: []
breaking: false
new_feature: false
bug_fix: true
---

Guard the seed-`Cargo.lock` copy in `TestWorkspace.generate()` on the file's existence. Previously, calling `TestWorkspace.subproject()` from a downstream consumer of the codegen-core JAR (where `aws/sdk/Cargo.lock` is not present on disk) would fail with `FileNotFoundException`. Inside the smithy-rs build the file is always present, so behavior is unchanged.
