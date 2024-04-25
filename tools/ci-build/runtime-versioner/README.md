# runtime-versioner
Runtime versioner serves two purposes:
1. `audit` the runtime crates to ensure that if their contents have changed, the version number in the crate has been updated. This is run as part of pre-commit.
2. `patch-runtime`: Used by `check-semver-hazards` (and manually) to test a specific set of runtime crates against the generated AWS SDK. This works by utilizing [Cargo's source patching](https://doc.rust-lang.org/cargo/reference/overriding-dependencies.html).
