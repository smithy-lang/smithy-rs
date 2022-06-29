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
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.withBlock
import software.amazon.smithy.rust.codegen.rustlang.withBlockTemplate
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.ServerRuntimeType
import software.amazon.smithy.rust.codegen.smithy.CoreCodegenContext
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.protocols.HttpBindingResolver
import software.amazon.smithy.rust.codegen.util.inputShape
import software.amazon.smithy.rust.codegen.util.toSnakeCase

/**
 * [ServerOperationRegistryGenerator] renders the `OperationRegistry` struct, a place where users can register their
 * service's operation implementations.
 *
 * Users can construct the operation registry using a builder. They can subsequently convert the operation registry into
 * the [`aws_smithy_http_server::Router`], a [`tower::Service`] that will route incoming requests to their operation
 * handlers, invoking them and returning the response.
 *
 * [`aws_smithy_http_server::Router`]: https://docs.rs/aws-smithy-http-server/latest/aws_smithy_http_server/struct.Router.html
 * [`tower::Service`]: https://docs.rs/tower/latest/tower/trait.Service.html
 */
class ServerOperationRegistryGenerator(
    coreCodegenContext: CoreCodegenContext,
    private val httpBindingResolver: HttpBindingResolver,
    private val operations: List<OperationShape>,
) {
    private val model = coreCodegenContext.model
    private val protocol = coreCodegenContext.protocol
    private val symbolProvider = coreCodegenContext.symbolProvider
    private val serviceName = coreCodegenContext.serviceShape.toShapeId().name
    private val operationNames = operations.map { symbolProvider.toSymbol(it).name.toSnakeCase() }
    private val runtimeConfig = coreCodegenContext.runtimeConfig
    private val codegenScope = arrayOf(
        "Router" to ServerRuntimeType.Router(runtimeConfig),
        "SmithyHttpServer" to ServerCargoDependency.SmithyHttpServer(runtimeConfig).asType(),
        "ServerOperationHandler" to ServerRuntimeType.OperationHandler(runtimeConfig),
        "Tower" to ServerCargoDependency.Tower.asType(),
        "Phantom" to ServerRuntimeType.Phantom,
        "StdError" to RuntimeType.StdError,
        "Display" to RuntimeType.Display,
        "From" to RuntimeType.From,
    )
    private val operationRegistryName = "OperationRegistry"
    private val operationRegistryBuilderName = "${operationRegistryName}Builder"
    private val operationRegistryErrorName = "${operationRegistryBuilderName}Error"
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

    private fun renderOperationRegistryStruct(writer: RustWriter) {
        writer.rustBlock("pub struct $operationRegistryNameWithArguments") {
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

    /**
     * Renders the `OperationRegistryBuilder` structure, used to build the `OperationRegistry`.
     */
    private fun renderOperationRegistryBuilderStruct(writer: RustWriter) {
        writer.rustBlock("pub struct $operationRegistryBuilderNameWithArguments") {
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

    /**
     * Renders the `OperationRegistryBuilderError` type, used to error out in case there are uninitialized fields.
     * This is an enum deriving `Debug` and implementing `Display` and `std::error::Error`.
     */
    private fun renderOperationRegistryBuilderError(writer: RustWriter) {
        Attribute.Derives(setOf(RuntimeType.Debug)).render(writer)
        writer.rustTemplate(
            """
            pub enum ${operationRegistryErrorName}{
                UninitializedField(&'static str)
            }
            impl #{Display} for ${operationRegistryErrorName}{
                fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    match self {
                        Self::UninitializedField(v) => write!(f, "{}", v),
                    }
                }
            }
            impl #{StdError} for $operationRegistryErrorName {}
            """,
            *codegenScope,
        )
    }

    /**
     * Renders the `OperationRegistryBuilder` `Default` implementation, used to create a new builder that can be
     * populated with the service's operation implementations.
     */
    private fun renderOperationRegistryBuilderDefault(writer: RustWriter) {
        writer.rustBlockTemplate("impl<$genericArguments> std::default::Default for $operationRegistryBuilderNameWithArguments") {
            val defaultOperations = operationNames.joinToString(separator = "\n,") { operationName ->
                "$operationName: Default::default()"
            }
            rustTemplate(
                """
                fn default() -> Self {
                    Self {
                        $defaultOperations,
                        _phantom: #{Phantom}
                    }
                }
                """,
                *codegenScope
            )
        }
    }

    /**
     * Renders the `OperationRegistryBuilder`'s impl block, where operations are stored.
     * The `build()` method converts the builder into an `OperationRegistry` instance.
     */
    private fun renderOperationRegistryBuilderImplementation(writer: RustWriter) {
        writer.rustBlock("impl<$genericArguments> $operationRegistryBuilderNameWithArguments") {
            operationNames.forEachIndexed { i, operationName ->
                rust(
                    """
                    pub fn $operationName(self, value: Op$i) -> Self {
                        let mut new = self;
                        new.$operationName = Some(value);
                        new
                    }
                    """
                )
            }

            rustBlock("pub fn build(self) -> Result<$operationRegistryNameWithArguments, ${operationRegistryErrorName}>") {
                withBlock("Ok( $operationRegistryName {", "})") {
                    for (operationName in operationNames) {
                        rust(
                            """
                            $operationName: match self.$operationName {
                                Some(v) => v,
                                None => return Err(${operationRegistryErrorName}::UninitializedField("$operationName")),
                            },
                            """
                        )
                    }
                    rustTemplate("_phantom: #{Phantom}", *codegenScope)
                }
            }
        }
    }

    /**
     * Renders the converter between the `OperationRegistry` and the `Router` via the `std::convert::From` trait.
     */
    private fun renderRouterImplementationFromOperationRegistryBuilder(writer: RustWriter) {
        val operationTraitBounds = writable {
            operations.forEachIndexed { i, operation ->
                rustTemplate(
                    """
                    Op$i: #{ServerOperationHandler}::Handler<B, In$i, #{OperationInput}>,
                    In$i: 'static + Send,
                    """,
                    *codegenScope,
                    "OperationInput" to symbolProvider.toSymbol(operation.inputShape(model))
                )
            }
        }

        writer.rustBlockTemplate(
            // The bound `B: Send` is required because of [`tower::util::BoxCloneService`].
            // [`tower::util::BoxCloneService`]: https://docs.rs/tower/latest/tower/util/struct.BoxCloneService.html#method.new
            """
            impl<$genericArguments> #{From}<$operationRegistryNameWithArguments> for #{Router}<B>
            where
                B: Send + 'static,
                #{operationTraitBounds:W}
            """,
            *codegenScope,
            "operationTraitBounds" to operationTraitBounds
        ) {
            rustBlock("fn from(registry: $operationRegistryNameWithArguments) -> Self") {
                val requestSpecsVarNames = operationNames.map { "${it}_request_spec" }

                requestSpecsVarNames.zip(operations).forEach { (requestSpecVarName, operation) ->
                    rustTemplate(
                        "let $requestSpecVarName = #{RequestSpec:W};",
                        "RequestSpec" to operation.requestSpec()
                    )
                }

                withBlockTemplate("#{Router}::${runtimeRouterConstructor()}(vec![", "])", *codegenScope) {
                    requestSpecsVarNames.zip(operationNames).forEach { (requestSpecVarName, operationName) ->
                        rustTemplate(
                            "(#{Tower}::util::BoxCloneService::new(#{ServerOperationHandler}::operation(registry.$operationName)), $requestSpecVarName),",
                            *codegenScope
                        )
                    }
                }
            }
        }
    }

    /**
     * Returns the `PhantomData` generic members in a comma-separated list.
     */
    private fun phantomMembers() = operationNames.mapIndexed { i, _ -> "In$i" }.joinToString(separator = ",\n")

    /**
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

    /**
     * Returns a writable for the `RequestSpec` for an operation based on the service's protocol.
     */
    private fun OperationShape.requestSpec(): Writable =
        when (protocol) {
            RestJson1Trait.ID, RestXmlTrait.ID -> restRequestSpec()
            AwsJson1_0Trait.ID, AwsJson1_1Trait.ID -> awsJsonOperationName()
            else -> TODO("Protocol $protocol not supported yet")
        }

    /**
     * Returns the operation name as required by the awsJson1.x protocols.
     */
    private fun OperationShape.awsJsonOperationName(): Writable {
        val operationName = symbolProvider.toSymbol(this).name
        return writable {
            rust("""String::from("$serviceName.$operationName")""")
        }
    }

    /**
     * Generates a restJson1 or restXml specific `RequestSpec`.
     */
    private fun OperationShape.restRequestSpec(): Writable {
        val httpTrait = httpBindingResolver.httpTrait(this)
        val extraCodegenScope =
            arrayOf("RequestSpec", "UriSpec", "PathAndQuerySpec", "PathSpec", "QuerySpec", "PathSegment", "QuerySegment").map {
                it to ServerCargoDependency.SmithyHttpServer(runtimeConfig).asType().member("routing::request_spec::$it")
            }.toTypedArray()

        // TODO(https://github.com/awslabs/smithy-rs/issues/950): Support the `endpoint` trait.
        val pathSegmentsVec = writable {
            withBlock("vec![", "]") {
                for (segment in httpTrait.uri.segments) {
                    val variant = when {
                        segment.isGreedyLabel -> "Greedy"
                        segment.isLabel -> "Label"
                        else -> """Literal(String::from("${segment.content}"))"""
                    }
                    rustTemplate(
                        "#{PathSegment}::$variant,",
                        *extraCodegenScope
                    )
                }
            }
        }

        val querySegmentsVec = writable {
            withBlock("vec![", "]") {
                for (queryLiteral in httpTrait.uri.queryLiterals) {
                    val variant = if (queryLiteral.value == "") {
                        """Key(String::from("${queryLiteral.key}"))"""
                    } else {
                        """KeyValue(String::from("${queryLiteral.key}"), String::from("${queryLiteral.value}"))"""
                    }
                    rustTemplate("#{QuerySegment}::$variant,", *extraCodegenScope)
                }
            }
        }

        return writable {
            rustTemplate(
                """
                #{RequestSpec}::new(
                    #{Method}::${httpTrait.method},
                    #{UriSpec}::new(
                        #{PathAndQuerySpec}::new(
                            #{PathSpec}::from_vector_unchecked(#{PathSegmentsVec:W}),
                            #{QuerySpec}::from_vector_unchecked(#{QuerySegmentsVec:W})
                        )
                    ),
                )
                """,
                *codegenScope,
                *extraCodegenScope,
                "PathSegmentsVec" to pathSegmentsVec,
                "QuerySegmentsVec" to querySegmentsVec,
                "Method" to CargoDependency.Http.asType().member("Method"),
            )
        }
    }
}
