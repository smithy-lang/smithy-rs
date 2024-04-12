/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.python.smithy.generators

import software.amazon.smithy.model.knowledge.TopDownIndex
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.traits.DocumentationTrait
import software.amazon.smithy.rust.codegen.core.rustlang.RustReservedWords
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.outputShape
import software.amazon.smithy.rust.codegen.core.util.toPascalCase
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase
import software.amazon.smithy.rust.codegen.server.python.smithy.PythonServerCargoDependency
import software.amazon.smithy.rust.codegen.server.python.smithy.PythonType
import software.amazon.smithy.rust.codegen.server.python.smithy.renderAsDocstring
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerProtocol
import software.amazon.smithy.rust.codegen.server.smithy.ServerRustModule.Error as ErrorModule
import software.amazon.smithy.rust.codegen.server.smithy.ServerRustModule.Input as InputModule
import software.amazon.smithy.rust.codegen.server.smithy.ServerRustModule.Output as OutputModule

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
class PythonApplicationGenerator(
    codegenContext: CodegenContext,
    private val protocol: ServerProtocol,
) {
    private val index = TopDownIndex.of(codegenContext.model)
    private val operations =
        index.getContainedOperations(codegenContext.serviceShape).toSortedSet(
            compareBy {
                it.id
            },
        ).toList()
    private val symbolProvider = codegenContext.symbolProvider
    private val libName = codegenContext.settings.moduleName.toSnakeCase()
    private val runtimeConfig = codegenContext.runtimeConfig
    private val service = codegenContext.serviceShape
    private val serviceName = service.id.name.toPascalCase()
    private val model = codegenContext.model
    private val codegenScope =
        arrayOf(
            "SmithyPython" to PythonServerCargoDependency.smithyHttpServerPython(runtimeConfig).toType(),
            "SmithyServer" to ServerCargoDependency.smithyHttpServer(runtimeConfig).toType(),
            "pyo3" to PythonServerCargoDependency.PyO3.toType(),
            "pyo3_asyncio" to PythonServerCargoDependency.PyO3Asyncio.toType(),
            "tokio" to PythonServerCargoDependency.Tokio.toType(),
            "tracing" to PythonServerCargoDependency.Tracing.toType(),
            "tower" to PythonServerCargoDependency.Tower.toType(),
            "tower_http" to PythonServerCargoDependency.TowerHttp.toType(),
            "num_cpus" to PythonServerCargoDependency.NumCpus.toType(),
            "hyper" to PythonServerCargoDependency.Hyper.toType(),
            "HashMap" to RuntimeType.HashMap,
            "parking_lot" to PythonServerCargoDependency.ParkingLot.toType(),
            "http" to RuntimeType.Http,
        )

    fun render(writer: RustWriter) {
        renderPyApplicationRustDocs(writer)
        renderAppStruct(writer)
        renderAppDefault(writer)
        renderAppClone(writer)
        renderPyAppTrait(writer)
        renderPyMethods(writer)
    }

    fun renderAppStruct(writer: RustWriter) {
        writer.rustTemplate(
            """
            ##[#{pyo3}::pyclass]
            ##[derive(Debug)]
            /// :generic Ctx:
            /// :extends typing.Generic\[Ctx\]:
            /// :rtype None:
            pub struct App {
                handlers: #{HashMap}<String, #{SmithyPython}::PyHandler>,
                middlewares: Vec<#{SmithyPython}::PyMiddlewareHandler>,
                context: Option<#{pyo3}::PyObject>,
                workers: #{parking_lot}::Mutex<Vec<#{pyo3}::PyObject>>,
            }
            """,
            *codegenScope,
        )
    }

    private fun renderAppClone(writer: RustWriter) {
        writer.rustTemplate(
            """
            impl Clone for App {
                fn clone(&self) -> Self {
                    Self {
                        handlers: self.handlers.clone(),
                        middlewares: self.middlewares.clone(),
                        context: self.context.clone(),
                        workers: #{parking_lot}::Mutex::new(vec![]),
                    }
                }
            }
            """,
            *codegenScope,
        )
    }

    private fun renderAppDefault(writer: RustWriter) {
        writer.rustTemplate(
            """
            impl Default for App {
                fn default() -> Self {
                    Self {
                        handlers: Default::default(),
                        middlewares: vec![],
                        context: None,
                        workers: #{parking_lot}::Mutex::new(vec![]),
                    }
                }
            }
            """,
            "Protocol" to protocol.markerStruct(),
            *codegenScope,
        )
    }

    private fun renderPyAppTrait(writer: RustWriter) {
        writer.rustBlockTemplate(
            """
            impl #{SmithyPython}::PyApp for App
            """,
            *codegenScope,
        ) {
            rustTemplate(
                """
                fn workers(&self) -> &#{parking_lot}::Mutex<Vec<#{pyo3}::PyObject>> {
                    &self.workers
                }
                fn context(&self) -> &Option<#{pyo3}::PyObject> {
                    &self.context
                }
                fn handlers(&mut self) -> &mut #{HashMap}<String, #{SmithyPython}::PyHandler> {
                    &mut self.handlers
                }
                """,
                *codegenScope,
            )

            rustBlockTemplate(
                """
                fn build_service(&mut self, event_loop: &#{pyo3}::PyAny) -> #{pyo3}::PyResult<
                    #{tower}::util::BoxCloneService<
                        #{http}::Request<#{SmithyServer}::body::Body>,
                        #{http}::Response<#{SmithyServer}::body::BoxBody>,
                        std::convert::Infallible
                    >
                >
                """,
                *codegenScope,
            ) {
                rustTemplate(
                    """
                    let builder = crate::service::$serviceName::builder_without_plugins();
                    """,
                    *codegenScope,
                )
                for (operation in operations) {
                    val fnName = RustReservedWords.escapeIfNeeded(symbolProvider.toSymbol(operation).name.toSnakeCase())
                    rustTemplate(
                        """
                        let ${fnName}_locals = #{pyo3_asyncio}::TaskLocals::new(event_loop);
                        let handler = self.handlers.get("$fnName").expect("Python handler for operation `$fnName` not found").clone();
                        let builder = builder.$fnName(move |input, state| {
                            #{pyo3_asyncio}::tokio::scope(${fnName}_locals.clone(), crate::python_operation_adaptor::$fnName(input, state, handler.clone()))
                        });
                        """,
                        *codegenScope,
                    )
                }
                rustTemplate(
                    """
                    let mut service = #{tower}::util::BoxCloneService::new(builder.build().expect("one or more operations do not have a registered handler; this is a bug in the Python code generator, please file a bug report under https://github.com/smithy-lang/smithy-rs/issues"));

                    {
                        use #{tower}::Layer;
                        #{tracing}::trace!("adding middlewares to rust python router");
                        let mut middlewares = self.middlewares.clone();
                        // Reverse the middlewares, so they run with same order as they defined
                        middlewares.reverse();
                        for handler in middlewares {
                            #{tracing}::trace!(name = &handler.name, "adding python middleware");
                            let locals = #{pyo3_asyncio}::TaskLocals::new(event_loop);
                            let layer = #{SmithyPython}::PyMiddlewareLayer::<#{Protocol}>::new(handler, locals);
                            service = #{tower}::util::BoxCloneService::new(layer.layer(service));
                        }
                    }
                    Ok(service)
                    """,
                    "Protocol" to protocol.markerStruct(),
                    *codegenScope,
                )
            }
        }
    }

    private fun renderPyMethods(writer: RustWriter) {
        writer.rustBlockTemplate(
            """
            ##[#{pyo3}::pymethods]
            impl App
            """,
            *codegenScope,
        ) {
            val middlewareRequest = PythonType.Opaque("Request", libName, rustNamespace = "crate::middleware")
            val middlewareResponse = PythonType.Opaque("Response", libName, rustNamespace = "crate::middleware")
            val middlewareNext = PythonType.Callable(listOf(middlewareRequest), PythonType.Awaitable(middlewareResponse))
            val middlewareFunc = PythonType.Callable(listOf(middlewareRequest, middlewareNext), PythonType.Awaitable(middlewareResponse))
            val tlsConfig = PythonType.Opaque("TlsConfig", libName, rustNamespace = "crate::tls")

            rustTemplate(
                """
                /// Create a new [App].
                ##[new]
                pub fn new() -> Self {
                    Self::default()
                }

                /// Register a context object that will be shared between handlers.
                ///
                /// :param context Ctx:
                /// :rtype ${PythonType.None.renderAsDocstring()}:
                ##[pyo3(text_signature = "(${'$'}self, context)")]
                pub fn context(&mut self, context: #{pyo3}::PyObject) {
                   self.context = Some(context);
                }

                /// Register a Python function to be executed inside a Tower middleware layer.
                ///
                /// :param func ${middlewareFunc.renderAsDocstring()}:
                /// :rtype ${PythonType.None.renderAsDocstring()}:
                ##[pyo3(text_signature = "(${'$'}self, func)")]
                pub fn middleware(&mut self, py: #{pyo3}::Python, func: #{pyo3}::PyObject) -> #{pyo3}::PyResult<()> {
                    let handler = #{SmithyPython}::PyMiddlewareHandler::new(py, func)?;
                    #{tracing}::trace!(
                        name = &handler.name,
                        is_coroutine = handler.is_coroutine,
                        "registering middleware function",
                    );
                    self.middlewares.push(handler);
                    Ok(())
                }

                /// Main entrypoint: start the server on multiple workers.
                ///
                /// :param address ${PythonType.Optional(PythonType.Str).renderAsDocstring()}:
                /// :param port ${PythonType.Optional(PythonType.Int).renderAsDocstring()}:
                /// :param backlog ${PythonType.Optional(PythonType.Int).renderAsDocstring()}:
                /// :param workers ${PythonType.Optional(PythonType.Int).renderAsDocstring()}:
                /// :param tls ${PythonType.Optional(tlsConfig).renderAsDocstring()}:
                /// :rtype ${PythonType.None.renderAsDocstring()}:
                ##[pyo3(text_signature = "(${'$'}self, address=None, port=None, backlog=None, workers=None, tls=None)")]
                pub fn run(
                    &mut self,
                    py: #{pyo3}::Python,
                    address: Option<String>,
                    port: Option<i32>,
                    backlog: Option<i32>,
                    workers: Option<usize>,
                    tls: Option<#{SmithyPython}::tls::PyTlsConfig>,
                ) -> #{pyo3}::PyResult<()> {
                    use #{SmithyPython}::PyApp;
                    self.run_server(py, address, port, backlog, workers, tls)
                }

                /// Lambda entrypoint: start the server on Lambda.
                ///
                /// :rtype ${PythonType.None.renderAsDocstring()}:
                ##[pyo3(text_signature = "(${'$'}self)")]
                pub fn run_lambda(
                    &mut self,
                    py: #{pyo3}::Python,
                ) -> #{pyo3}::PyResult<()> {
                    use #{SmithyPython}::PyApp;
                    self.run_lambda_handler(py)
                }

                /// Build the service and start a single worker.
                ##[pyo3(text_signature = "(${'$'}self, socket, worker_number, tls=None)")]
                pub fn start_worker(
                    &mut self,
                    py: pyo3::Python,
                    socket: &pyo3::PyCell<#{SmithyPython}::PySocket>,
                    worker_number: isize,
                    tls: Option<#{SmithyPython}::tls::PyTlsConfig>,
                ) -> pyo3::PyResult<()> {
                    use #{SmithyPython}::PyApp;
                    let event_loop = self.configure_python_event_loop(py)?;
                    let service = self.build_and_configure_service(py, event_loop)?;
                    self.start_hyper_worker(py, socket, event_loop, service, worker_number, tls)
                }
                """,
                *codegenScope,
            )
            operations.map { operation ->
                val operationName = symbolProvider.toSymbol(operation).name
                val fnName = RustReservedWords.escapeIfNeeded(symbolProvider.toSymbol(operation).name.toSnakeCase())

                val input = PythonType.Opaque("${operationName}Input", libName, rustNamespace = "crate::input")
                val output = PythonType.Opaque("${operationName}Output", libName, rustNamespace = "crate::output")
                val context = PythonType.Opaque("Ctx", libName)
                val returnType = PythonType.Union(listOf(output, PythonType.Awaitable(output)))
                val handler =
                    PythonType.Union(
                        listOf(
                            PythonType.Callable(
                                listOf(input, context),
                                returnType,
                            ),
                            PythonType.Callable(
                                listOf(input),
                                returnType,
                            ),
                        ),
                    )

                rustTemplate(
                    """
                    /// Method to register `$fnName` Python implementation inside the handlers map.
                    /// It can be used as a function decorator in Python.
                    ///
                    /// :param func ${handler.renderAsDocstring()}:
                    /// :rtype ${PythonType.None.renderAsDocstring()}:
                    ##[pyo3(text_signature = "(${'$'}self, func)")]
                    pub fn $fnName(&mut self, py: #{pyo3}::Python, func: #{pyo3}::PyObject) -> #{pyo3}::PyResult<()> {
                        use #{SmithyPython}::PyApp;
                        self.register_operation(py, "$fnName", func)
                    }
                    """,
                    *codegenScope,
                )
            }
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
            /// from $libName import ${InputModule.name}
            /// from $libName import ${OutputModule.name}
            """.trimIndent(),
        )
        if (operations.any { it.errors.isNotEmpty() }) {
            writer.rust("""/// from $libName import ${ErrorModule.name}""".trimIndent())
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

    private fun RustWriter.operationImplementationStubs(operations: List<OperationShape>) =
        rust(
            operations.joinToString("\n///\n") {
                val operationDocumentation = it.getTrait<DocumentationTrait>()?.value
                val ret =
                    if (!operationDocumentation.isNullOrBlank()) {
                        operationDocumentation.replace("#", "##").prependIndent("/// ## ") + "\n"
                    } else {
                        ""
                    }
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
        val inputT = "${InputModule.name}::${inputSymbol.name}"
        val outputT = "${OutputModule.name}::${outputSymbol.name}"
        val operationName = symbolProvider.toSymbol(this).name.toSnakeCase()
        return "@app.$operationName\n/// def $operationName(input: $inputT, ctx: Context) -> $outputT"
    }
}
