sdk-lockfiles
=============

This CLI tool audits the `Cargo.lock` files in the `smithy-rs` repository. These lockfiles are used to ensure
reproducible builds. The `sdk-lockfiles` tool specifically audits the following lockfiles:
- The [lockfile](https://github.com/smithy-lang/smithy-rs/blob/main/rust-runtime/Cargo.lock) for Smithy runtime crates
- The [lockfile](https://github.com/smithy-lang/smithy-rs/blob/main/aws/rust-runtime/Cargo.lock) for AWS runtime crates
- The [lockfile](https://github.com/smithy-lang/smithy-rs/blob/main/aws/rust-runtime/aws-config/Cargo.lock) for the `aws-config` crate
- The [lockfile](https://github.com/smithy-lang/smithy-rs/blob/main/aws/sdk/Cargo.lock) for the workspace containing code-generated AWS SDK crates (*)

Specifically, the tool ensures that the lockfile marked with (*) is a superset containing all dependencies listed in
the rest of the runtime lockfiles. If it detects a new dependency in the AWS SDK crates introduced by any of the runtime
lockfiles (unless the dependency is introduced by a server runtime crate), it will output a message similar to the
following:
```
$ sdk-lockfiles audit
2024-09-10T16:48:38.460518Z  INFO sdk_lockfiles::audit: checking whether `rust-runtime/Cargo.lock` is covered by the SDK lockfile...
2024-09-10T16:48:38.489879Z  INFO sdk_lockfiles::audit: checking whether `aws/rust-runtime/Cargo.lock` is covered by the SDK lockfile...
2024-09-10T16:48:38.490306Z  INFO sdk_lockfiles::audit: checking whether `aws/rust-runtime/aws-config/Cargo.lock` is covered by the SDK lockfile...
`minicbor` (0.24.2), used by `rust-runtime/Cargo.lock`, is not contained in SDK lockfile!
Error: there are lockfile audit failures
```

This tool is intended for automated use.
