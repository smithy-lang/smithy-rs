# Generating Common Service Code

How a service is constructed and how to plug in new business logic is described in [Pokémon Service][1].
This document introduces the project and how code is being generated. It is written for developers who want to start contributing to `smithy-rs`.

## Folder structure

The project is divided in:

- `/codegen`: it contains shared code for both client and server, but only generates a client
- `/codegen-server`: server only. This project started with `codegen` to generate a client, but client and server share common code; that code lives in `codegen`, which `codegen-server` depends on
- `/aws`: the AWS Rust SDK, it deals with AWS services specifically. The folder structure reflects the project's, with the `rust-runtime` and the `codegen`
- `/rust-runtime`: the generated client and server crates may depend on crates in this folder. Crates here are not code generated. The only crate that is not published is `inlineable`,
which contains common functions used by other crates, [copied into][2] the source crate

`/rust-runtime` crates ("runtime crates") are added to a crate's dependency only when used. If a model uses event streams, it will depend on [`aws-smithy-eventstream`][3].

## Generating code

`smithy-rs`'s entry points are Smithy code-generation plugins, and is not a command. One entry point is in [RustCodegenPlugin::execute][4] and
inherits from `SmithyBuildPlugin` in [smithy-build][5]. Code generation is in Kotlin and shared common, non-Rust specific code with the [`smithy` Java repository][6]. They plug into the [Smithy gradle][7] plugin, which is a gradle plugin.

The comment at the beginning of `execute` describes what a `Decorator` is and uses the following terms:

- Context: contains the model being generated, projection and settings for the build
- Decorator: (also referred to as customizations) customizes how code is being generated. AWS services are required to sign with the SigV4 protocol, and [a decorator][8] adds Rust code to sign requests and responses.
  Decorators are applied in reverse order of being added and have a priority order.
- Writer: creates files and adds content; it supports templating, using `#` for substitutions
- Location: the file where a symbol will be written to

The only task of a `RustCodegenPlugin` is to construct a `CodegenVisitor` and call its [execute()][9] method.

`CodegenVisitor::execute()` is given a `Context` and decorators, and calls a [CodegenVisitor][10].

CodegenVisitor, RustCodegenPlugin, and wherever there are different implementations between client and server, such as in generating error types,
have corresponding server versions.

Objects used throughout code generation are:

- Symbol: a node in a graph, an abstraction that represents the qualified name of a type; symbols reference and depend on other symbols, and have some common properties among languages (such as a namespace or a definition file). For Rust, we add properties to include more metadata about a symbol, such as its [type][11]
- [RustType][12]: `Option<T>`, `HashMap`, ... along with their namespaces of origin such as `std::collections`
- [RuntimeType][13]: the information to locate a type, plus the crates it depends on
- [ShapeId][14]: an immutable object that identifies a `Shape`

Useful conversions are:

```kotlin
SymbolProvider.toSymbol(shape)
```

where `SymbolProvider` constructs symbols for shapes. Some symbols require to create other symbols and types;
[event streams][15] and [other streaming shapes][16] are an example.
Symbol providers are all [applied][17] in order; if a shape uses a reserved keyword in Rust, its name is converted to a new name by a [symbol provider][18],
and all other providers will work with this [new][19] symbol.

```kotlin
Model.expectShape(shapeId)
```

Each model has a `shapeId` to `shape` map; this method returns the shape associated with this shapeId.

Some objects implement a `transform` [method][20] that only change the input model, so that code generation will work on that new model. This is used to, for example, add a trait to a shape.

`CodegenVisitor` is a `ShapeVisitor`. For all services in the input model, shapes are [converted into Rust][21];
[here][22] is how a service is constructed,
[here][23] a structure and so on.

Code generation flows from writer to files and entities are (mostly) generated only on a [need-by-need basis][24].
The complete result is a [Rust crate][25],
in which all dependencies are written into their modules and `lib.rs` is generated ([here][26]).
`execute()` ends by running [cargo fmt][27],
to avoid having to format correctly Rust in `Writer`s and to be sure the generated code follows the styling rules.

[1]: ./pokemon_service.md
[2]: https://github.com/awslabs/smithy-rs/blob/db48039065bec890ef387385773b37154b555b14/codegen/src/main/kotlin/software/amazon/smithy/rust/codegen/rustlang/CargoDependency.kt#L95-L95
[3]: https://docs.rs/aws-smithy-eventstream
[4]: https://github.com/awslabs/smithy-rs/blob/db48039065bec890ef387385773b37154b555b14/codegen/src/main/kotlin/software/amazon/smithy/rust/codegen/smithy/RustCodegenPlugin.kt#L34
[5]: https://github.com/awslabs/smithy/tree/main/smithy-build
[6]: https://github.com/awslabs/smithy
[7]: https://awslabs.github.io/smithy/1.0/guides/building-models/gradle-plugin.html
[8]: https://github.com/awslabs/smithy-rs/blob/db48039065bec890ef387385773b37154b555b14/aws/sdk-codegen/src/main/kotlin/software/amazon/smithy/rustsdk/SigV4SigningDecorator.kt#L45
[9]: https://github.com/awslabs/smithy-rs/blob/db48039065bec890ef387385773b37154b555b14/codegen/src/main/kotlin/software/amazon/smithy/rust/codegen/smithy/CodegenVisitor.kt#L115-L115
[10]: https://github.com/awslabs/smithy-rs/blob/db48039065bec890ef387385773b37154b555b14/codegen/src/main/kotlin/software/amazon/smithy/rust/codegen/smithy/CodegenVisitor.kt#L44
[11]: https://github.com/awslabs/smithy-rs/blob/db48039065bec890ef387385773b37154b555b14/codegen/src/main/kotlin/software/amazon/smithy/rust/codegen/smithy/SymbolVisitor.kt#L363-L363
[12]: https://github.com/awslabs/smithy-rs/blob/db48039065bec890ef387385773b37154b555b14/codegen/src/main/kotlin/software/amazon/smithy/rust/codegen/rustlang/RustTypes.kt#L25-L25
[13]: https://github.com/awslabs/smithy-rs/blob/db48039065bec890ef387385773b37154b555b14/codegen/src/main/kotlin/software/amazon/smithy/rust/codegen/smithy/RuntimeTypes.kt#L113-L113
[14]: https://awslabs.github.io/smithy/1.0/spec/core/model.html#shape-id
[15]: https://github.com/awslabs/smithy-rs/blob/db48039065bec890ef387385773b37154b555b14/codegen/src/main/kotlin/software/amazon/smithy/rust/codegen/smithy/EventStreamSymbolProvider.kt#L65-L65
[16]: https://github.com/awslabs/smithy-rs/blob/db48039065bec890ef387385773b37154b555b14/codegen/src/main/kotlin/software/amazon/smithy/rust/codegen/smithy/StreamingTraitSymbolProvider.kt#L26-L26
[17]: https://github.com/awslabs/smithy-rs/blob/db48039065bec890ef387385773b37154b555b14/codegen/src/main/kotlin/software/amazon/smithy/rust/codegen/smithy/RustCodegenPlugin.kt#L62-L62
[18]: https://github.com/awslabs/smithy-rs/blob/db48039065bec890ef387385773b37154b555b14/codegen/src/main/kotlin/software/amazon/smithy/rust/codegen/rustlang/RustReservedWords.kt#L26-L26
[19]: https://github.com/awslabs/smithy-rs/blob/db48039065bec890ef387385773b37154b555b14/codegen/src/main/kotlin/software/amazon/smithy/rust/codegen/smithy/EventStreamSymbolProvider.kt#L38-L38
[20]: https://github.com/awslabs/smithy-rs/blob/db48039065bec890ef387385773b37154b555b14/codegen/src/main/kotlin/software/amazon/smithy/rust/codegen/smithy/transformers/OperationNormalizer.kt#L52-L52
[21]: https://github.com/awslabs/smithy-rs/blob/db48039065bec890ef387385773b37154b555b14/codegen/src/main/kotlin/software/amazon/smithy/rust/codegen/smithy/CodegenVisitor.kt#L119-L119
[22]: https://github.com/awslabs/smithy-rs/blob/db48039065bec890ef387385773b37154b555b14/codegen/src/main/kotlin/software/amazon/smithy/rust/codegen/smithy/CodegenVisitor.kt#L150-L150
[23]: https://github.com/awslabs/smithy-rs/blob/db48039065bec890ef387385773b37154b555b14/codegen/src/main/kotlin/software/amazon/smithy/rust/codegen/smithy/CodegenVisitor.kt#L172-L172
[24]: https://github.com/awslabs/smithy-rs/blob/db48039065bec890ef387385773b37154b555b14/codegen/src/main/kotlin/software/amazon/smithy/rust/codegen/smithy/CodegenDelegator.kt#L119-L126
[25]: https://github.com/awslabs/smithy-rs/blob/db48039065bec890ef387385773b37154b555b14/codegen/src/main/kotlin/software/amazon/smithy/rust/codegen/smithy/CodegenDelegator.kt#L42-L42
[26]: https://github.com/awslabs/smithy-rs/blob/db48039065bec890ef387385773b37154b555b14/codegen/src/main/kotlin/software/amazon/smithy/rust/codegen/smithy/CodegenDelegator.kt#L96-L107
[27]: https://github.com/awslabs/smithy-rs/blob/db48039065bec890ef387385773b37154b555b14/codegen/src/main/kotlin/software/amazon/smithy/rust/codegen/smithy/CodegenVisitor.kt#L133-L133
