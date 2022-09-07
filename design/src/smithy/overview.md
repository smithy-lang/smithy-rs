# Smithy
The Rust SDK uses Smithy models and code generation tooling to generate an SDK. Smithy is an open source IDL (interface design language) developed by Amazon. Although the Rust SDK uses Smithy models for AWS services, smithy-rs and Smithy models in general are not AWS specific.

Design documentation here covers both our implementation of Smithy Primitives (e.g. simple shape) as well as more complex Smithy traits like [Endpoint](./endpoint.md).

## Internals
Smithy introduces a few concepts that are defined here:

1. Shape: The core Smithy primitive. A smithy model is composed of nested shapes defining an API.
2. `Symbol`: A Representation of a type including namespaces & and any dependencies required to use a type. A shape can be converted into a symbol by a `SymbolVisitor`. A `SymbolVisitor` maps shapes to types in your programming language (e.g. Rust). In the Rust SDK, see [SymbolVisitor.kt](https://github.com/awslabs/smithy-rs/blob/c049a37f8cba5f9bec2e96c28db83e7efb2edc53/codegen/src/main/kotlin/software/amazon/smithy/rust/codegen/smithy/SymbolVisitor.kt). Symbol visitors are composable—many specific behaviors are mixed in via small & focused symbol providers, e.g. support for the streaming trait is mixed in separately.
3. `Writer`: Writers are code generation primitives that collect code prior to being written to a file. Writers enable language specific helpers to be added to simplify codegen for a given language. For example, `smithy-rs` adds `rustBlock` to [`RustWriter`](https://github.com/awslabs/smithy-rs/blob/908dec558e26bbae6fe4b7d9d1c221dd81699b59/codegen/src/main/kotlin/software/amazon/smithy/rust/codegen/rustlang/RustWriter.kt) to create a "Rust block" of code.
   ```kotlin
   writer.rustBlock("struct Model") {
       model.fields.forEach {
           write("${field.name}: #T", field.symbol)
       }
   }
   ```
   This would produce something like:
   ```rust
   struct Model {
      field1: u32,
      field2: String
   }
   ```

4. Generators: A Generator, e.g. `StructureGenerator`, `UnionGenerator` generates more complex Rust code from a Smithy model. Protocol generators pull these individual tools together to generate code for an entire service / protocol.

A developer's view of code generation can be found in [this document](../docs/code_generation.md).
