# aws-smithy-lang

Load, build, inspect, validate, traverse, and serialize Smithy 2.0 models from
the [Smithy IDL](https://smithy.io/2.0/spec/idl.html) and
[JSON AST](https://smithy.io/2.0/spec/json-ast.html) representations.

This crate is unstable. A `Model` is an immutable set of owned semantic shapes.
`ModelAssembler` builds one from any mix of Smithy 2.0 IDL and JSON AST
documents, existing models, and shapes built in code, merges them, injects the Smithy
prelude, and validates the result with the built-in `StructuralV1` validators
plus any custom `Validator`s. `AstWriter` writes JSON AST in insertion order or
canonical order. Every loaded shape, member, trait, and node value records its
source location (file, line, column, and semantic JSON Pointer), and errors and
diagnostics report it. Inputs that use `apply` or mixins are rejected with an
`UnsupportedFeature` diagnostic.

With the optional `runtime-schema` feature, `runtime_schema::RuntimeSchemaSet`
converts a `Model` into `aws-smithy-schema` `Schema`s, the descriptors that
generated clients use for schema-based serialization, so a client can work with
a model loaded at runtime. The conversion validates schema-relevant trait values
and reports located errors, builds each operation its own input schema with the
operation's `@http` binding, and can be limited to one service. The feature adds
a dependency on `aws-smithy-schema` and, through it, the Smithy client runtime.

<!-- anchor_start:footer -->
This crate is part of the [AWS SDK for Rust](https://awslabs.github.io/aws-sdk-rust/) and the [smithy-rs](https://github.com/smithy-lang/smithy-rs) code generator. In most cases, it should not be used directly.
<!-- anchor_end:footer -->
