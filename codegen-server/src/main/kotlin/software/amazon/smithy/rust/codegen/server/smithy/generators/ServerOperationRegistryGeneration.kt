/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.rustlang.Attribute
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.server.smithy.ServerRuntimeType
import software.amazon.smithy.rust.codegen.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.protocols.HttpBindingResolver
import software.amazon.smithy.rust.codegen.util.dq
import software.amazon.smithy.rust.codegen.util.toSnakeCase

/**
 * ServerOperationRegistryGenerator
 */
class ServerOperationRegistryGenerator(
    codegenContext: CodegenContext,
    private val httpBindingResolver: HttpBindingResolver,
    private val operations: List<OperationShape>,
) {
    private val serverCrate = "aws_smithy_http_server"
    private val service = codegenContext.serviceShape
    private val model = codegenContext.model
    private val symbolProvider = codegenContext.symbolProvider
    private val operationNames = operations.map { symbolProvider.toSymbol(it).name.toSnakeCase() }
    private val runtimeConfig = codegenContext.runtimeConfig
    private val codegenScope = arrayOf(
        "Router" to ServerRuntimeType.Router(runtimeConfig),
        "SmithyHttpServer" to CargoDependency.SmithyHttpServer(runtimeConfig).asType(),
        "Phantom" to ServerRuntimeType.Phantom,
        "StdError" to RuntimeType.StdError
    )
    private val operationRegistryName = "${service.getContextualName(service)}OperationRegistry"
    private val operationRegistryBuilderName = "${operationRegistryName}Builder"
    private val genericArguments = "B, " + operations.mapIndexed { i, _ -> "Op$i, In$i" }.joinToString()
    private val operationRegistryNameWithArguments = "$operationRegistryName<$genericArguments>"
    private val operationRegistryBuilderNameWithArguments = "$operationRegistryBuilderName<$genericArguments>"

    fun render(writer: RustWriter) {
        renderOperationRegistryStruct(writer)
        renderOperationRegistryBuilderStruct(writer)
        renderOperationRegistryBuilderError(writer)
        renderOperationRegistryBuilderDefault(writer)
        renderOperationRegistryBuilderImpl(writer)
        renderRouterImplFromOperationRegistryBuilder(writer)
    }

    /*
     * Renders the OperationRegistry structure, holding all the operations and their generic inputs.
     */
    private fun renderOperationRegistryStruct(writer: RustWriter) {
        // A lot of things can become pretty complex in this type as it will hold 2 generics per operation
        Attribute.Custom("allow(clippy::all)").render(writer)
        writer.rustBlock(
            """
            pub struct $operationRegistryNameWithArguments
            """.trimIndent()
        ) {
            val members = operationNames
                .mapIndexed { i, operationName -> "$operationName: Op$i" }
                .joinToString(separator = ",\n")
            rustTemplate(
                """
                $members,
                _phantom: #{Phantom}<(B, ${phantomMembers()})>,
                """,
                *codegenScope
            )
        }
    }

    /*
     * Renders the OperationRegistryBuilder structure, used to build the OperationRegistry and then convert it
     * into a Smithy Router.
     */
    private fun renderOperationRegistryBuilderStruct(writer: RustWriter) {
        // A lot of things can become pretty complex in this type as it will hold 2 generics per operation
        Attribute.Custom("allow(clippy::all)").render(writer)
        writer.rustBlock(
            """
            pub struct $operationRegistryBuilderNameWithArguments
            """.trimIndent()
        ) {
            val members = operationNames
                .mapIndexed { i, operationName -> "$operationName: Option<Op$i>" }
                .joinToString(separator = ",\n")
            rustTemplate(
                """
                $members,
                _phantom: #{Phantom}<(B, ${phantomMembers()})>,
                """,
                *codegenScope
            )
        }
    }

    /*
     * Renders the OperationRegistryBuilder Error type, used to error out in case there are uninitialized fields.
     * This structs implement Debug, Display and std::error::Error.
     */
    private fun renderOperationRegistryBuilderError(writer: RustWriter) {
        // derive[Debug] is needed to impl std::error::Error
        Attribute.Derives(setOf(RuntimeType.Debug)).render(writer)
        writer.rustTemplate(
            """
            pub enum ${operationRegistryBuilderName}Error {
                UninitializedField(&'static str)
            }
            impl std::fmt::Display for ${operationRegistryBuilderName}Error {
                fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    match self {
                        Self::UninitializedField(v) => write!(f, "{}", v),
                    }
                }
            }
            impl #{StdError} for ${operationRegistryBuilderName}Error {}
            """.trimIndent(),
            *codegenScope
        )
    }

    /*
     * Renders the OperationRegistryBuilder Default implementation , used to create a new builder that can be
     * later filled with operations and their routees.
     */
    private fun renderOperationRegistryBuilderDefault(writer: RustWriter) {
        writer.rustBlockTemplate(
            """
            impl<$genericArguments> Default for $operationRegistryBuilderNameWithArguments
            """.trimIndent()
        ) {
            val defaultOperations = operationNames.map { operationName ->
                "$operationName: Default::default()"
            }.joinToString(separator = "\n,")
            rustTemplate(
                """
                fn default() -> Self {
                    Self { $defaultOperations, _phantom: #{Phantom} }
                }
                """,
                *codegenScope
            )
        }
    }

    /*
     * Renders the OperationRegistryBuilder implementation, where operations and their routes
     * are stored. The build() method converts the builder into a real OperationRegistry instance.
     */
    private fun renderOperationRegistryBuilderImpl(writer: RustWriter) {
        // A lot of things can become pretty complex in this type as it will hold 2 generics per operation
        Attribute.Custom("allow(clippy::all)").render(writer)
        writer.rustBlockTemplate(
            """
            impl<$genericArguments> $operationRegistryBuilderNameWithArguments
            """.trimIndent(),
            *codegenScope
        ) {
            val registerOperations = operationNames.mapIndexed { i, operationName ->
                """pub fn $operationName(self, value: Op$i) -> Self {
                let mut new = self;
                new.$operationName = Some(value);
                new
                }"""
            }.joinToString(separator = "\n")
            val registerOperationsBuilder = operationNames.map { operationName ->
                """
                $operationName: match self.$operationName {
                    Some(v) => v,
                    None => return Err(${operationRegistryBuilderName}Error::UninitializedField(${operationName.dq()})),
                }
                """
            }.joinToString(separator = "\n,")
            rustTemplate(
                """
                $registerOperations
                pub fn build(self) -> Result<$operationRegistryNameWithArguments, ${operationRegistryBuilderName}Error> {
                    Ok($operationRegistryName { $registerOperationsBuilder, _phantom: #{Phantom} })
                }
                """,
                *codegenScope
            )
        }
    }

    /*
     * Renders the conversion between the OperationRegistry and the Router via the into() method.
     */
    private fun renderRouterImplFromOperationRegistryBuilder(writer: RustWriter) {
        // A lot of things can become pretty complex in this type as it will hold 2 generics per operation
        val operationsTraitBounds = operations
            .mapIndexed { i, operation ->
                val operationName = symbolProvider.toSymbol(operation).name
                """Op$i: crate::operation_handler::Handler<B, In$i, crate::input::${operationName}Input>,
            In$i: 'static"""
            }.joinToString(separator = ",\n")
        Attribute.Custom("allow(clippy::all)").render(writer)
        writer.rustBlockTemplate(
            """
            impl<$genericArguments> From<$operationRegistryNameWithArguments> for #{Router}<B>
            where
                B: Send + 'static,
                $operationsTraitBounds
            """.trimIndent(),
            *codegenScope
        ) {
            rustBlock("fn from(registry: $operationRegistryNameWithArguments) -> Self") {
                val requestSpecsVarNames = operationNames.map { "${it}_request_spec" }
                val routes = requestSpecsVarNames.zip(operationNames) { requestSpecVarName, operationName ->
                    ".route($requestSpecVarName, crate::operation_handler::operation(registry.$operationName))"
                }.joinToString(separator = "\n")

                val requestSpecs = requestSpecsVarNames.zip(operations) { requestSpecVarName, operation ->
                    "let $requestSpecVarName = ${operation.requestSpec()};"
                }.joinToString(separator = "\n")
                rustTemplate(
                    """
                    $requestSpecs
                    #{Router}::new()
                        $routes
                    """.trimIndent(),
                    *codegenScope
                )
            }
        }
    }

    /*
     * Renders the PhantomData generic members.
     */
    private fun phantomMembers(): String {
        return operationNames
            .mapIndexed { i, _ -> "In$i" }
            .joinToString(separator = ",\n")
    }

    /*
     * Generate the requestSpecs for an operation based on its route.
     */
    private fun OperationShape.requestSpec(): String {
        val httpTrait = httpBindingResolver.httpTrait(this)
        val namespace = ServerRuntimeType.RequestSpecModule(runtimeConfig).fullyQualifiedName()

        // TODO: Support the `endpoint` trait: https://awslabs.github.io/smithy/1.0/spec/core/endpoint-traits.html#endpoint-trait
        val pathSegments = httpTrait.uri.segments.map {
            "$namespace::PathSegment::" +
                if (it.isGreedyLabel) "Greedy"
                else if (it.isLabel) "Label"
                else "Literal(String::from(\"${it.content}\"))"
        }
        val querySegments = httpTrait.uri.queryLiterals.map {
            "$namespace::QuerySegment::" +
                if (it.value == "") "Key(String::from(\"${it.key}\"))"
                else "KeyValue(String::from(\"${it.key}\"), String::from(\"${it.value}\"))"
        }

        return """
            $namespace::RequestSpec::new(
                http::Method::${httpTrait.method},
                $namespace::UriSpec {
                    host_prefix: None,
                    path_and_query: $namespace::PathAndQuerySpec {
                        path_segments: $namespace::PathSpec::from_vector_unchecked(vec![${pathSegments.joinToString()}]),
                        query_segments: $namespace::QuerySpec::from_vector_unchecked(vec![${querySegments.joinToString()}])
                    }
                }
            )
        """.trimIndent()
    }
}
