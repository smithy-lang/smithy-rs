sdk-versioner
============

This is a CLI tool that will recursively update all references to the AWS Rust SDK
in Cargo.toml files in a given directory. That is, it finds every Cargo.toml file nested
within that directory, and modifies dependency lines that point to the AWS Rust SDK
or its supporting crates. These dependencies can be updated to be based on file-system
path, crates.io version, or both.

Example updating SDK examples to use SDK version 0.5.0 with Smithy version 0.35.0:
```bash
$ sdk-versioner \
  --sdk-version 0.5.0 \
  --smithy-version 0.35.0 \
  path/to/aws-doc-sdk-examples/rust_dev_preview
```

Example updating SDK examples to refer to local generated code:
```bash
$ sdk-versioner \
  --sdk-path path/to/smithy-rs/aws/sdk/build/aws-sdk/sdk \
  path/to/aws-doc-sdk-examples/rust_dev_preview
```
