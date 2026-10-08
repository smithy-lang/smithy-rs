# aws-smithy-lang

Load, inspect, traverse, and serialize [Smithy 2.0 JSON AST](https://smithy.io/2.0/spec/json-ast.html) models.

This crate is unstable. Version `0.1` loads a single Smithy 2.0 JSON AST document, injects the
Smithy prelude, validates it under the `StructuralV1` profile, and writes deterministic JSON AST.
Documents that use `apply` or mixins are rejected with an `UnsupportedFeature` diagnostic.

<!-- anchor_start:footer -->
This crate is part of the [AWS SDK for Rust](https://awslabs.github.io/aws-sdk-rust/) and the [smithy-rs](https://github.com/smithy-lang/smithy-rs) code generator. In most cases, it should not be used directly.
<!-- anchor_end:footer -->
