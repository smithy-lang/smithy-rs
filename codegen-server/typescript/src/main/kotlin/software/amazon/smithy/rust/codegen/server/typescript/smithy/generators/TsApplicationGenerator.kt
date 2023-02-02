/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.typescript.smithy.generators

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.traits.DocumentationTrait
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute.Companion.derive
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.ErrorsModule
import software.amazon.smithy.rust.codegen.core.smithy.InputsModule
import software.amazon.smithy.rust.codegen.core.smithy.OutputsModule
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.outputShape
import software.amazon.smithy.rust.codegen.core.util.toPascalCase
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerProtocol
import software.amazon.smithy.rust.codegen.server.typescript.smithy.TsServerCargoDependency

/**
 * Generates a Python compatible application and server that can be configured from Python.
 *
 * Example:
 *     from pool import DatabasePool
 *     from my_library import App, OperationInput, OperationOutput
 *     @dataclass
 *     class Context:
 *         db = DatabasePool()
 *
 *     app = App()
 *     app.context(Context())
 *
 *     @app.operation
 *     def operation(input: OperationInput, ctx: State) -> OperationOutput:
 *        description = await ctx.db.get_description(input.name)
 *        return OperationOutput(description)
 *
 *     app.run()
 *
 * The application holds a mapping between operation names (lowercase, snakecase),
 * the context as defined in Python and some task local with the Python event loop
 * for the current process.
 *
 * The application exposes several methods to Python:
 * * `App()`: constructor to create an instance of `App`.
 * * `run()`: run the application on a number of workers.
 * * `context()`: register the context object that is passed to the Python handlers.
 * * One register method per operation that can be used as decorator. For example if
 *   the model has one operation called `RegisterServer`, it will codegenerate a method
 *   of `App` called `register_service()` that can be used to decorate the Python implementation
 *   of this operation.
 *
 * This class also renders the implementation of the `aws_smithy_http_server_python::PyServer` trait,
 * that abstracts the processes / event loops / workers lifecycles.
 */
class TsApplicationGenerator(
    codegenContext: CodegenContext,
    private val protocol: ServerProtocol,
    private val operations: List<OperationShape>,
) {
    private val symbolProvider = codegenContext.symbolProvider
    private val libName = codegenContext.settings.moduleName.toSnakeCase()
    private val runtimeConfig = codegenContext.runtimeConfig
    private val service = codegenContext.serviceShape
    private val serviceName = service.id.name.toPascalCase()
    private val model = codegenContext.model
    private val codegenScope =
        arrayOf(
            "SmithyPython" to TsServerCargoDependency.smithyHttpServerPython(runtimeConfig).toType(),
            "SmithyTs" to TsServerCargoDependency.smithyHttpServerTs(runtimeConfig).toType(),
            "SmithyServer" to ServerCargoDependency.smithyHttpServer(runtimeConfig).toType(),
            "pyo3" to TsServerCargoDependency.PyO3.toType(),
            "pyo3_asyncio" to TsServerCargoDependency.PyO3Asyncio.toType(),
            "napi" to TsServerCargoDependency.Napi.toType(),
            "napi_derive" to TsServerCargoDependency.NapiDerive.toType(),
            "tokio" to TsServerCargoDependency.Tokio.toType(),
            "tracing" to TsServerCargoDependency.Tracing.toType(),
            "tower" to TsServerCargoDependency.Tower.toType(),
            "tower_http" to TsServerCargoDependency.TowerHttp.toType(),
            "num_cpus" to TsServerCargoDependency.NumCpus.toType(),
            "hyper" to TsServerCargoDependency.Hyper.toType(),
            "HashMap" to RuntimeType.HashMap,
            "parking_lot" to TsServerCargoDependency.ParkingLot.toType(),
            "http" to RuntimeType.Http,
        )

    fun render(writer: RustWriter) {
        writer.write("use napi_derive::napi;")
        renderHandlers(writer)
        // renderPyApplicationRustDocs(writer)
        renderApp(writer)
    }

    fun renderHandlers(writer: RustWriter) {
        Attribute(derive(RuntimeType.Clone)).render(writer)
        writer.rustBlock("""pub struct Handlers""") {
            operations.map { operation ->
                val operationName = symbolProvider.toSymbol(operation).name
                val input = "crate::input::${operationName}Input"
                val fnName = operationName.toSnakeCase()
                rustTemplate(
                    """
                    pub(crate) $fnName: #{napi}::threadsafe_function::ThreadsafeFunction<
                        $input,
                        #{napi}::threadsafe_function::ErrorStrategy::CalleeHandled
                    >,
                    """,
                    *codegenScope,
                )
            }
        }
        Attribute("napi(object)").render(writer)
        writer.rustBlock("pub struct TsHandlers") {
            operations.map { operation ->
                val operationName = symbolProvider.toSymbol(operation).name
                val input = "crate::input::${operationName}Input"
                val output = "crate::output::${operationName}Output"
                val error = "crate::error::${operationName}Error"
                val fnName = operationName.toSnakeCase()
                rustTemplate(
                    """
                    ##[napi(ts_type = "(input: $input) => Promise<$output|$error>")]
                    pub $fnName: #{napi}::JsFunction,
                    """,
                    *codegenScope,
                )
            }
        }
    }

    private fun renderApp(writer: RustWriter) {
        Attribute("napi").render(writer)
        writer.rust(
            """
            pub struct App {
                handlers: Handlers,
            }
            """,
        )
        Attribute("napi").render(writer)
        writer.rustBlock("impl App") {
            renderAppCreate(writer)
            renderAppStart(writer)
        }
    }

    private fun renderAppCreate(writer: RustWriter) {
        Attribute("napi(constructor)").render(writer)
        writer.rustBlockTemplate(
            """pub fn create(js_handlers: TsHandlers) -> #{napi}::Result<Self>""",
            *codegenScope,
        ) {
            operations.map { operation ->
                val operationName = symbolProvider.toSymbol(operation).name
                val input = "crate::input::${operationName}Input"
                val fnName = operationName.toSnakeCase()
                rustTemplate(
                    """
                    let $fnName: #{napi}::threadsafe_function::ThreadsafeFunction<
                        $input, #{napi}::threadsafe_function::ErrorStrategy::CalleeHandled
                    > = js_handlers.$fnName.create_threadsafe_function(0, |ctx| Ok(vec![ctx.value]))?;
                    """,
                    *codegenScope,
                )
            }
            rust("let handlers = Handlers {")
            operations.map { operation ->
                val operationName = symbolProvider.toSymbol(operation).name.toSnakeCase()
                rust("    $operationName: $operationName.clone(),")
            }
            rust("};")
            writer.rust("Ok(Self{ handlers })")
        }
    }

    private fun renderAppStart(writer: RustWriter) {
        Attribute("napi").render(writer)
        writer.rustBlockTemplate(
            """pub fn start(&self, socket: &#{SmithyTs}::socket::TsSocket) -> #{napi}::Result<()>""",
            *codegenScope,
        ) {
            rustTemplate(
                """
                // #{SmithyTs}::setup_tracing();
                let plugins = #{SmithyServer}::plugin::PluginPipeline::new();
                let builder = crate::service::PokemonService::builder_with_plugins(plugins);
                """,
                *codegenScope,
            )
            operations.map { operation ->
                val operationName = symbolProvider.toSymbol(operation).name.toSnakeCase()
                rust("let builder = builder.$operationName(crate::js_operation_adaptor::$operationName);")
            }
            rustTemplate(
                """
                let app = builder.build().expect("failed to build instance of PokemonService")
                    .layer(&#{SmithyServer}::AddExtensionLayer::new(self.handlers.clone()));
                let service = #{tower}::util::BoxCloneService::new(app);
                #{SmithyTs}::server::start_hyper_worker(socket, service).expect("failed to start the hyper server");
                Ok(())
                """,
                *codegenScope,
            )
        }
    }

    private fun renderPyApplicationRustDocs(writer: RustWriter) {
        writer.rust(
            """
            ##[allow(clippy::tabs_in_doc_comments)]
            /// Main Python application, used to register operations and context and start multiple
            /// workers on the same shared socket.
            ///
            /// Operations can be registered using the application object as a decorator (`@app.operation_name`).
            ///
            /// Here's a full example to get you started:
            ///
            /// ```python
            """.trimIndent(),
        )
        writer.rust(
            """
            /// from $libName import ${InputsModule.name}
            /// from $libName import ${OutputsModule.name}
            """.trimIndent(),
        )
        if (operations.any { it.errors.isNotEmpty() }) {
            writer.rust("""/// from $libName import ${ErrorsModule.name}""".trimIndent())
        }
        writer.rust(
            """
            /// from $libName import middleware
            /// from $libName import App
            ///
            /// @dataclass
            /// class Context:
            ///     counter: int = 0
            ///
            /// app = App()
            /// app.context(Context())
            ///
            /// @app.request_middleware
            /// def request_middleware(request: middleware::Request):
            ///     if request.get_header("x-amzn-id") != "secret":
            ///         raise middleware.MiddlewareException("Unsupported `x-amz-id` header", 401)
            ///
            """.trimIndent(),
        )
        writer.operationImplementationStubs(operations)
        writer.rust(
            """
            ///
            /// app.run()
            /// ```
            ///
            /// Any of operations above can be written as well prepending the `async` keyword and
            /// the Python application will automatically handle it and schedule it on the event loop for you.
            """.trimIndent(),
        )
    }

    private fun RustWriter.operationImplementationStubs(operations: List<OperationShape>) = rust(
        operations.joinToString("\n///\n") {
            val operationDocumentation = it.getTrait<DocumentationTrait>()?.value
            val ret = if (!operationDocumentation.isNullOrBlank()) {
                operationDocumentation.replace("#", "##").prependIndent("/// ## ") + "\n"
            } else ""
            ret +
                """
                /// ${it.signature()}:
                ///     raise NotImplementedError
                """.trimIndent()
        },
    )

    /**
     * Returns the function signature for an operation handler implementation. Used in the documentation.
     */
    private fun OperationShape.signature(): String {
        val inputSymbol = symbolProvider.toSymbol(inputShape(model))
        val outputSymbol = symbolProvider.toSymbol(outputShape(model))
        val inputT = "${InputsModule.name}::${inputSymbol.name}"
        val outputT = "${OutputsModule.name}::${outputSymbol.name}"
        val operationName = symbolProvider.toSymbol(this).name.toSnakeCase()
        return "@app.$operationName\n/// def $operationName(input: $inputT, ctx: Context) -> $outputT"
    }
}
