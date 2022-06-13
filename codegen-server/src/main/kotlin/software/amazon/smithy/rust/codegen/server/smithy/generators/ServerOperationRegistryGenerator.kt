/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.aws.traits.protocols.AwsJson1_0Trait
import software.amazon.smithy.aws.traits.protocols.AwsJson1_1Trait
import software.amazon.smithy.aws.traits.protocols.RestJson1Trait
import software.amazon.smithy.aws.traits.protocols.RestXmlTrait
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.rustlang.Attribute
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
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
    private val protocol = codegenContext.protocol
    private val symbolProvider = codegenContext.symbolProvider
    private val serviceName = codegenContext.serviceShape.toShapeId().name
    private val operationNames = operations.map { symbolProvider.toSymbol(it).name.toSnakeCase() }
    private val runtimeConfig = codegenContext.runtimeConfig
    private val codegenScope = arrayOf(
        "Router" to ServerRuntimeType.Router(runtimeConfig),
        "SmithyHttpServer" to ServerCargoDependency.SmithyHttpServer(runtimeConfig).asType(),
        "ServerOperationHandler" to ServerRuntimeType.OperationHandler(runtimeConfig),
        "Tower" to ServerCargoDependency.Tower.asType(),
        "Phantom" to ServerRuntimeType.Phantom,
        "StdError" to RuntimeType.StdError
    )
    private val operationRegistryName = "OperationRegistry"
    private val operationRegistryBuilderName = "OperationRegistryBuilder"
    private val genericArguments = "B, " + operations.mapIndexed { i, _ -> "Op$i, In$i" }.joinToString()
    private val operationRegistryNameWithArguments = "$operationRegistryName<$genericArguments>"
    private val operationRegistryBuilderNameWithArguments = "$operationRegistryBuilderName<$genericArguments>"

    fun render(writer: RustWriter) {
        renderOperationRegistryStruct(writer)
        renderOperationRegistryBuilderStruct(writer)
        renderOperationRegistryBuilderError(writer)
        renderOperationRegistryBuilderDefault(writer)
        renderOperationRegistryBuilderImplementation(writer)
        renderRouterImplementationFromOperationRegistryBuilder(writer)
    }

    /*
     * Renders the `OperationRegistry` structure, holding all the operations and their generic inputs.
     */
    private fun renderOperationRegistryStruct(writer: RustWriter) {
        // A lot of things can become pretty complex in this type as it will hold 2 generics per operation.
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
     * Renders the `OperationRegistryBuilder` structure, used to build the `OperationRegistry`, which can then be converted into a Smithy router.
     */
    private fun renderOperationRegistryBuilderStruct(writer: RustWriter) {
        // A lot of things can become pretty complex in this type as it will hold 2 generics per operation.
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
     * Renders the `OperationRegistryBuilderError` type, used to error out in case there are uninitialized fields.
     * This enum implement `Debug`, `Display` and `std::error::Error`.
     */
    private fun renderOperationRegistryBuilderError(writer: RustWriter) {
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
            *codegenScope,
            "Display" to RuntimeType.Display,
        )
    }

    /*
     * Renders the `OperationRegistryBuilder` `Default` implementation, used to create a new builder that can be
     * populated with the service's operations.
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
     * Renders the `OperationRegistryBuilder` implementation, where operations are stored.
     * The `build()` method converts the builder into an `OperationRegistry` instance.
     */
    private fun renderOperationRegistryBuilderImplementation(writer: RustWriter) {
        // A lot of things can become pretty complex in this type as it will hold 2 generics per operation.
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
     * Renders the conversion code between the `OperationRegistry` and the `Router` via the `std::convert::From` trait.
     */
    private fun renderRouterImplementationFromOperationRegistryBuilder(writer: RustWriter) {
        // A lot of things can become pretty complex in this type as it will hold 2 generics per operation.
        val operationsTraitBounds = operations
            .mapIndexed { i, operation ->
                val operationName = symbolProvider.toSymbol(operation).name
                """Op$i: #{ServerOperationHandler}::Handler<B, In$i, crate::input::${operationName}Input>,
                In$i: 'static + Send"""
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
                val requestSpecs = requestSpecsVarNames.zip(operations) { requestSpecVarName, operation ->
                    "let $requestSpecVarName = ${operation.requestSpec()};"
                }.joinToString(separator = "\n")
                val towerServices = requestSpecsVarNames.zip(operationNames) { requestSpecVarName, operationName ->
                    "(#{Tower}::util::BoxCloneService::new(#{ServerOperationHandler}::operation(registry.$operationName)), $requestSpecVarName)"
                }.joinToString(prefix = "vec![", separator = ",\n", postfix = "]")

                rustTemplate(
                    """
                    $requestSpecs
                    #{Router}::${runtimeRouterConstructor()}($towerServices)
                    """.trimIndent(),
                    *codegenScope
                )
            }
        }
    }

    /*
     * Renders the `PhantomData` generic members.
     */
    private fun phantomMembers(): String {
        return operationNames
            .mapIndexed { i, _ -> "In$i" }
            .joinToString(separator = ",\n")
    }

    /*
     * Finds the runtime function to construct a new `Router` based on the Protocol.
     */
    private fun runtimeRouterConstructor(): String =
        when (protocol) {
            RestJson1Trait.ID -> "new_rest_json_router"
            RestXmlTrait.ID -> "new_rest_xml_router"
            AwsJson1_0Trait.ID -> "new_aws_json_10_router"
            AwsJson1_1Trait.ID -> "new_aws_json_11_router"
            else -> TODO("Protocol $protocol not supported yet")
        }

    /*
     * Returns the `RequestSpec`s for an operation based on its HTTP-bound route.
     */
    private fun OperationShape.requestSpec(): String =
        when (protocol) {
            RestJson1Trait.ID, RestXmlTrait.ID -> restRequestSpec()
            AwsJson1_0Trait.ID, AwsJson1_1Trait.ID -> awsJsonOperationName()
            else -> TODO("Protocol $protocol not supported yet")
        }

    /*
     * Returns an AwsJson specific runtime `RequestSpec`.
     */
    private fun OperationShape.awsJsonOperationName(): String {
        val operationName = symbolProvider.toSymbol(this).name
        // TODO(https://github.com/awslabs/smithy-rs/issues/950): Support the `endpoint` trait: https://awslabs.github.io/smithy/1.0/spec/core/endpoint-traits.html#endpoint-trait
        return """String::from("$serviceName.$operationName")"""
    }

    /*
     * Generates a REST (RestJson1, RestXml) specific runtime `RequestSpec`.
     */
    private fun OperationShape.restRequestSpec(): String {
        val httpTrait = httpBindingResolver.httpTrait(this)
        val namespace = ServerRuntimeType.RequestSpecModule(runtimeConfig).fullyQualifiedName()
        // TODO(https://github.com/awslabs/smithy-rs/issues/950): Support the `endpoint` trait.
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
                $namespace::UriSpec::new(
                    $namespace::PathAndQuerySpec::new(
                        $namespace::PathSpec::from_vector_unchecked(vec![${pathSegments.joinToString()}]),
                        $namespace::QuerySpec::from_vector_unchecked(vec![${querySegments.joinToString()}])
                    )
                ),
            )
        """.trimIndent()
    }
}
