/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.traits.DocumentationTrait
import software.amazon.smithy.rust.codegen.rustlang.Attribute
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.DependencyScope
import software.amazon.smithy.rust.codegen.rustlang.RustReservedWords
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
import software.amazon.smithy.rust.codegen.smithy.Errors
import software.amazon.smithy.rust.codegen.smithy.Inputs
import software.amazon.smithy.rust.codegen.smithy.Outputs
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.generators.CodegenTarget
import software.amazon.smithy.rust.codegen.smithy.generators.error.errorSymbol
import software.amazon.smithy.rust.codegen.smithy.protocols.Protocol
import software.amazon.smithy.rust.codegen.util.getTrait
import software.amazon.smithy.rust.codegen.util.inputShape
import software.amazon.smithy.rust.codegen.util.outputShape
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
    private val coreCodegenContext: CoreCodegenContext,
    private val protocol: Protocol,
    private val operations: List<OperationShape>,
) {
    private val crateName = coreCodegenContext.settings.moduleName
    private val model = coreCodegenContext.model
    private val symbolProvider = coreCodegenContext.symbolProvider
    private val serviceName = coreCodegenContext.serviceShape.toShapeId().name
    private val operationNames = operations.map { RustReservedWords.escapeIfNeeded(symbolProvider.toSymbol(it).name.toSnakeCase()) }
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
        renderOperationRegistryRustDocs(writer)
        renderOperationRegistryStruct(writer)
        renderOperationRegistryBuilderStruct(writer)
        renderOperationRegistryBuilderError(writer)
        renderOperationRegistryBuilderDefault(writer)
        renderOperationRegistryBuilderImplementation(writer)
        renderRouterImplementationFromOperationRegistryBuilder(writer)
    }

    private fun renderOperationRegistryRustDocs(writer: RustWriter) {
        val inputOutputErrorsImport = if (operations.any { it.errors.isNotEmpty() }) {
            "/// use ${crateName.toSnakeCase()}::{${Inputs.namespace}, ${Outputs.namespace}, ${Errors.namespace}};"
        } else {
            "/// use ${crateName.toSnakeCase()}::{${Inputs.namespace}, ${Outputs.namespace}};"
        }

        writer.rustTemplate(
"""
##[allow(clippy::tabs_in_doc_comments)]
/// The `$operationRegistryName` is the place where you can register
/// your service's operation implementations.
///
/// Use [`$operationRegistryBuilderName`] to construct the
/// `$operationRegistryName`. For each of the [operations] modeled in
/// your Smithy service, you need to provide an implementation in the
/// form of a Rust async function or closure that takes in the
/// operation's input as their first parameter, and returns the
/// operation's output. If your operation is fallible (i.e. it
/// contains the `errors` member in your Smithy model), the function
/// implementing the operation has to be fallible (i.e. return a
/// [`Result`]). **You must register an implementation for all
/// operations with the correct signature**, or your application
/// will fail to compile.
///
/// The operation registry can be converted into an [`#{Router}`] for
/// your service. This router will take care of routing HTTP
/// requests to the matching operation implementation, adhering to
/// your service's protocol and the [HTTP binding traits] that you
/// used in your Smithy model. This router can be converted into a
/// type implementing [`tower::make::MakeService`], a _service
/// factory_. You can feed this value to a [Hyper server], and the
/// server will instantiate and [`serve`] your service.
///
/// Here's a full example to get you started:
///
/// ```rust
/// use std::net::SocketAddr;
$inputOutputErrorsImport
/// use ${crateName.toSnakeCase()}::operation_registry::$operationRegistryBuilderName;
/// use #{Router};
///
/// ##[#{Tokio}::main]
/// pub async fn main() {
///    let app: Router = $operationRegistryBuilderName::default()
${operationNames.map { ".$it($it)" }.joinToString("\n") { it.prependIndent("///        ") }}
///        .build()
///        .expect("unable to build operation registry")
///        .into();
///
///    let bind: SocketAddr = format!("{}:{}", "127.0.0.1", "6969")
///        .parse()
///        .expect("unable to parse the server bind address and port");
///
///    let server = #{Hyper}::Server::bind(&bind).serve(app.into_make_service());
///
///    // Run your service!
///    // if let Err(err) = server.await {
///    //   eprintln!("server error: {}", err);
///    // }
/// }
///
${operationImplementationStubs(operations)}
/// ```
///
/// [`serve`]: https://docs.rs/hyper/0.14.16/hyper/server/struct.Builder.html##method.serve
/// [`tower::make::MakeService`]: https://docs.rs/tower/latest/tower/make/trait.MakeService.html
/// [HTTP binding traits]: https://awslabs.github.io/smithy/1.0/spec/core/http-traits.html
/// [operations]: https://awslabs.github.io/smithy/1.0/spec/core/model.html##operation
/// [Hyper server]: https://docs.rs/hyper/latest/hyper/server/index.html
""",
            "Router" to ServerRuntimeType.Router(runtimeConfig),
            // These should be dev-dependencies. Not all sSDKs depend on `Hyper` (only those that convert the body
            // `to_bytes`), and none depend on `tokio`.
            "Tokio" to ServerCargoDependency.TokioDev.asType(),
            "Hyper" to CargoDependency.Hyper.copy(scope = DependencyScope.Dev).asType(),
        )
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
                *codegenScope,
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
                *codegenScope,
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
            pub enum $operationRegistryErrorName {
                UninitializedField(&'static str)
            }
            impl #{Display} for $operationRegistryErrorName {
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
                *codegenScope,
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
                    """,
                )
            }

            rustBlock("pub fn build(self) -> Result<$operationRegistryNameWithArguments, $operationRegistryErrorName>") {
                withBlock("Ok( $operationRegistryName {", "})") {
                    for (operationName in operationNames) {
                        rust(
                            """
                            $operationName: match self.$operationName {
                                Some(v) => v,
                                None => return Err($operationRegistryErrorName::UninitializedField("$operationName")),
                            },
                            """,
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
                    Op$i: #{ServerOperationHandler}::Handler<B, In$i, ${symbolProvider.toSymbol(operation.inputShape(model)).fullName}>,
                    In$i: 'static + Send,
                    """,
                    *codegenScope,
                    "OperationInput" to symbolProvider.toSymbol(operation.inputShape(model)),
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
            "operationTraitBounds" to operationTraitBounds,
        ) {
            rustBlock("fn from(registry: $operationRegistryNameWithArguments) -> Self") {
                val requestSpecsVarNames = operationNames.map { "${it}_request_spec" }

                requestSpecsVarNames.zip(operations).forEach { (requestSpecVarName, operation) ->
                    rustTemplate(
                        "let $requestSpecVarName = #{RequestSpec:W};",
                        "RequestSpec" to operation.requestSpec(),
                    )
                }

                val sensitivityGens = operations.map {
                    ServerHttpSensitivityGenerator(model, it, coreCodegenContext.runtimeConfig)
                }

                withBlockTemplate(
                    "#{Router}::${protocol.serverRouterRuntimeConstructor()}(vec![",
                    "])",
                    *codegenScope,
                ) {
                    requestSpecsVarNames.zip(operationNames).zip(sensitivityGens).forEach {
                        val (inner, sensitivityGen) = it
                        val (requestSpecVarName, operationName) = inner

                        rustBlock("") {
                            rustTemplate("let svc = #{ServerOperationHandler}::operation(registry.$operationName);", *codegenScope)
                            withBlock("let request_fmt =", ";") {
                                sensitivityGen.renderRequestFmt(writer)
                            }
                            withBlock("let response_fmt =", ";") {
                                sensitivityGen.renderResponseFmt(writer)
                            }
                            rustTemplate("let svc = #{SmithyHttpServer}::logging::InstrumentOperation::new(svc, \"$operationName\").request_fmt(request_fmt).response_fmt(response_fmt);", *codegenScope)
                            rustTemplate("(#{Tower}::util::BoxCloneService::new(svc), $requestSpecVarName)", *codegenScope)
                        }
                        rust(",")
                    }
                }
            }
        }
    }

    /**
     * Returns the `PhantomData` generic members in a comma-separated list.
     */
    private fun phantomMembers() = operationNames.mapIndexed { i, _ -> "In$i" }.joinToString(separator = ",\n")

    private fun operationImplementationStubs(operations: List<OperationShape>): String =
        operations.joinToString("\n///\n") {
            val operationDocumentation = it.getTrait<DocumentationTrait>()?.value
            val ret = if (!operationDocumentation.isNullOrBlank()) {
                operationDocumentation.replace("#", "##").prependIndent("/// /// ") + "\n"
            } else ""
            ret +
                """
                /// ${it.signature()} {
                ///     todo!()
                /// }
                """.trimIndent()
        }

    /**
     * Returns the function signature for an operation handler implementation. Used in the documentation.
     */
    private fun OperationShape.signature(): String {
        val inputSymbol = symbolProvider.toSymbol(inputShape(model))
        val outputSymbol = symbolProvider.toSymbol(outputShape(model))
        val errorSymbol = errorSymbol(model, symbolProvider, CodegenTarget.SERVER)

        val inputT = "${Inputs.namespace}::${inputSymbol.name}"
        val t = "${Outputs.namespace}::${outputSymbol.name}"
        val outputT = if (errors.isEmpty()) {
            t
        } else {
            val e = "${Errors.namespace}::${errorSymbol.name}"
            "Result<$t, $e>"
        }

        val operationName = RustReservedWords.escapeIfNeeded(symbolProvider.toSymbol(this).name.toSnakeCase())
        return "async fn $operationName(input: $inputT) -> $outputT"
    }

    /**
     * Returns a writable for the `RequestSpec` for an operation based on the service's protocol.
     */
    private fun OperationShape.requestSpec(): Writable = protocol.serverRouterRequestSpec(
        this,
        symbolProvider.toSymbol(this).name,
        serviceName,
        ServerCargoDependency.SmithyHttpServer(runtimeConfig).asType().member("routing::request_spec"),
    )
}
