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
import software.amazon.smithy.rust.codegen.server.smithy.protocols.ServerHttpProtocolGenerator
import software.amazon.smithy.rust.codegen.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.protocols.HttpBindingResolver
import software.amazon.smithy.rust.codegen.smithy.protocols.HttpTraitHttpBindingResolver
import software.amazon.smithy.rust.codegen.smithy.protocols.ProtocolContentTypes
import software.amazon.smithy.rust.codegen.util.dq
import software.amazon.smithy.rust.codegen.util.toSnakeCase

/**
 * ServerOperationRegistryGenerator
 */
class ServerOperationRegistryGenerator(
    codegenContext: CodegenContext,
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
        "Display" to RuntimeType.Display,
        "StdError" to RuntimeType.StdError
    )
    private val httpBindingResolver: HttpBindingResolver =
        HttpTraitHttpBindingResolver(codegenContext.model, ProtocolContentTypes.consistent("application/json"))

    fun render(writer: RustWriter) {
        // Registry
        Attribute.Derives(setOf(RuntimeType.Debug)).render(writer)
        val genericArguments = operations.mapIndexed { i, _ -> "Op$i, In$i" }.joinToString()
        val operationsGenericArguments = "B, $genericArguments"
        val operationRegistryName = "${service.getContextualName(service)}OperationRegistry"
        val operationRegistryNameWithArguments = "$operationRegistryName<$operationsGenericArguments>"
        writer.rustBlock(
            """
            pub struct $operationRegistryNameWithArguments
            """.trimIndent()
        ) {
            val members = operationNames
                .mapIndexed { i, operationName -> "$operationName: Op$i" }
                .joinToString(separator = ",\n")
            val phantomMembers = operationNames
                .mapIndexed { i, _ -> "In$i" }
                .joinToString(separator = ",\n")
            rust(
                """
                $members,
                _phantom: std::marker::PhantomData<(B, $phantomMembers)>,
                """
            )
        }
        // Builder error
        val operationRegistryBuilderName = "${operationRegistryName}Builder"
        val operationRegistryBuilderNameWithArguments = "$operationRegistryBuilderName<$operationsGenericArguments>"
        Attribute.Derives(setOf(RuntimeType.Debug)).render(writer)
        writer.rustTemplate(
            """
            pub enum ${operationRegistryBuilderName}Error {
                UninitializedField(&'static str)
            }
            impl #{Display} for ${operationRegistryBuilderName}Error {
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
        // Builder
        writer.rustBlock(
            """
            pub struct $operationRegistryBuilderNameWithArguments
            """.trimIndent()
        ) {
            val members = operationNames
                .mapIndexed { i, operationName -> "$operationName: Option<Op$i>" }
                .joinToString(separator = ",\n")
            val phantomMembers = operationNames
                .mapIndexed { i, _ -> "In$i" }
                .joinToString(separator = ",\n")
            rust(
                """
                $members,
                _phantom: std::marker::PhantomData<(B, $phantomMembers)>,
                """
            )
        }
        // Builder default
        writer.rustBlockTemplate(
            """
            impl<$operationsGenericArguments> Default for $operationRegistryBuilderNameWithArguments
            """.trimIndent()
        ) {
            val defaultOperations = operationNames.map { operationName ->
                "$operationName: Default::default()"
            }.joinToString(separator = "\n,")
            rustTemplate(
                """
                fn default() -> Self {
                    Self { $defaultOperations, _phantom: std::marker::PhantomData }
                }
                """.trimIndent()
            )
        }
        // Builder impl
        writer.rustBlockTemplate(
            """
            impl<$operationsGenericArguments> $operationRegistryBuilderNameWithArguments
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
                    Ok($operationRegistryName { $registerOperationsBuilder, _phantom: std::marker::PhantomData })
                }
                """.trimIndent(),
                *codegenScope
            )
        }

        writer.rustBlockTemplate(
            """
            impl<$operationsGenericArguments> From<$operationRegistryNameWithArguments> for #{Router}<B>
            where
                B: Send + 'static,
                ${operationsTraitBounds()}
            """.trimIndent(),
            *codegenScope
        ) {
            rustBlock("fn from(registry: $operationRegistryNameWithArguments) -> Self") {
                val operationInOutWrappers = operations.map {
                    val operationName = symbolProvider.toSymbol(it).name
                    Pair(
                        "crate::operation::$operationName${ServerHttpProtocolGenerator.OPERATION_INPUT_WRAPPER_SUFFIX}",
                        "crate::operation::$operationName${ServerHttpProtocolGenerator.OPERATION_OUTPUT_WRAPPER_SUFFIX}"
                    )
                }
                val requestSpecsVarNames = operationNames.map { "${it}_request_spec" }
                val routes = requestSpecsVarNames.zip(operationNames).zip(operationInOutWrappers) { (requestSpecVarName, operationName), (inputWrapper, outputWrapper) ->
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

    private fun operationsTraitBounds(): String = operations
        .mapIndexed { i, operation ->
            val operationName = symbolProvider.toSymbol(operation).name
            val inputName = "crate::input::${operationName}Input"
            """Op$i: crate::operation_handler::Handler<B, In$i, $inputName>,
            In$i: 'static"""
        }.joinToString(separator = ",\n")

    private fun OperationShape.requestSpec(): String {
        val httpTrait = httpBindingResolver.httpTrait(this)
        val namespace = ServerRuntimeType.RequestSpecModule(runtimeConfig).fullyQualifiedName()

        // TODO Support the `endpoint` trait: https://awslabs.github.io/smithy/1.0/spec/core/endpoint-traits.html#endpoint-trait

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
