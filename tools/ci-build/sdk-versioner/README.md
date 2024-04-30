sdk-versioner
============

This is a CLI tool that will recursively update all references to the AWS Rust SDK
in Cargo.toml files in a given directory. That is, it finds every Cargo.toml file nested
within that directory, and modifies dependency lines that point to the AWS Rust SDK
or its supporting crates. These dependencies can be updated to be based on file-system
path, crates.io version, or both.

This tool is currently used to update all the dependencies of all examples to refer to the latest SDK version. See [aws/sdk/build.gradle.kts](../../../aws/sdk/build.gradle.kts) for more context.
