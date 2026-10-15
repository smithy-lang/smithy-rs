# aws-smithy-lang

Load, build, inspect, validate, traverse, and serialize Smithy 2.0 models in
[JSON AST](https://smithy.io/2.0/spec/json-ast.html) form.

This crate is unstable. A `Model` is an immutable set of owned semantic shapes.
`ModelAssembler` builds one from any mix of Smithy 2.0 JSON AST documents,
existing models, and shapes built in code, merges them, injects the Smithy
prelude, and validates the result with the built-in `StructuralV1` validators
plus any custom `Validator`s. `AstWriter` writes JSON AST in insertion order or
canonical order. Every loaded shape, member, trait, and node value records its
source location. Inputs that use `apply` or mixins are rejected with an
`UnsupportedFeature` diagnostic.

<!-- anchor_start:footer -->
This crate is part of the [AWS SDK for Rust](https://awslabs.github.io/aws-sdk-rust/) and the [smithy-rs](https://github.com/smithy-lang/smithy-rs) code generator. In most cases, it should not be used directly.
<!-- anchor_end:footer -->
