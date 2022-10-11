/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.python.smithy.generators

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.traits.DocumentationTrait
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.asType
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.Errors
import software.amazon.smithy.rust.codegen.core.smithy.Inputs
import software.amazon.smithy.rust.codegen.core.smithy.Outputs
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.outputShape
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase
import software.amazon.smithy.rust.codegen.server.python.smithy.PythonServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerProtocol

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
 * This class also renders the implementation of the `aws_smity_http_server_python::PyServer` trait,
 * that abstracts the processes / event loops / workers lifecycles.
 */
class PythonApplicationGenerator(
    codegenContext: CodegenContext,
    private val protocol: ServerProtocol,
    private val operations: List<OperationShape>,
) {
    private val symbolProvider = codegenContext.symbolProvider
    private val libName = "lib${codegenContext.settings.moduleName.toSnakeCase()}"
    private val runtimeConfig = codegenContext.runtimeConfig
    private val model = codegenContext.model
    private val codegenScope =
        arrayOf(
            "SmithyPython" to PythonServerCargoDependency.SmithyHttpServerPython(runtimeConfig).asType(),
            "SmithyServer" to ServerCargoDependency.SmithyHttpServer(runtimeConfig).asType(),
            "pyo3" to PythonServerCargoDependency.PyO3.asType(),
            "pyo3_asyncio" to PythonServerCargoDependency.PyO3Asyncio.asType(),
            "tokio" to PythonServerCargoDependency.Tokio.asType(),
            "tracing" to PythonServerCargoDependency.Tracing.asType(),
            "tower" to PythonServerCargoDependency.Tower.asType(),
            "tower_http" to PythonServerCargoDependency.TowerHttp.asType(),
            "num_cpus" to PythonServerCargoDependency.NumCpus.asType(),
            "hyper" to PythonServerCargoDependency.Hyper.asType(),
            "HashMap" to RustType.HashMap.RuntimeType,
            "parking_lot" to PythonServerCargoDependency.ParkingLot.asType(),
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
            pub struct App {
                handlers: #{HashMap}<String, #{SmithyPython}::PyHandler>,
                middlewares: #{SmithyPython}::PyMiddlewares,
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
                        middlewares: #{SmithyPython}::PyMiddlewares::new::<#{Protocol}>(vec![]),
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
                fn middlewares(&mut self) -> &mut #{SmithyPython}::PyMiddlewares {
                    &mut self.middlewares
                }
                """,
                *codegenScope,
            )

            rustBlockTemplate(
                """
                // Dynamically codegenerate the routes, allowing to build the Smithy [#{SmithyServer}::routing::Router].
                fn build_router(&mut self, event_loop: &#{pyo3}::PyAny) -> #{pyo3}::PyResult<#{SmithyServer}::routing::Router>
                """,
                *codegenScope,
            ) {
                rustTemplate(
                    """
                    let router = crate::operation_registry::OperationRegistryBuilder::default();
                    """,
                    *codegenScope,
                )
                for (operation in operations) {
                    val operationName = symbolProvider.toSymbol(operation).name
                    val name = operationName.toSnakeCase()
                    rustTemplate(
                        """
                        let ${name}_locals = #{pyo3_asyncio}::TaskLocals::new(event_loop);
                        let handler = self.handlers.get("$name").expect("Python handler for operation `$name` not found").clone();
                        let router = router.$name(move |input, state| {
                            #{pyo3_asyncio}::tokio::scope(${name}_locals, crate::operation_handler::$name(input, state, handler))
                        });
                        """,
                        *codegenScope,
                    )
                }
                rustTemplate(
                    """
                    let middleware_locals = pyo3_asyncio::TaskLocals::new(event_loop);
                    let service = #{tower}::ServiceBuilder::new()
                        .layer(
                            #{SmithyPython}::PyMiddlewareLayer::<#{Protocol}>::new(self.middlewares.clone(), middleware_locals),
                        );
                    let router: #{SmithyServer}::routing::Router = router
                        .build()
                        .expect("Unable to build operation registry")
                        .into();
                    Ok(router.layer(service))
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
            rustTemplate(
                """
                /// Create a new [App].
                ##[new]
                pub fn new() -> Self {
                    Self::default()
                }
                /// Register a context object that will be shared between handlers.
                ##[pyo3(text_signature = "(${'$'}self, context)")]
                pub fn context(&mut self, context: #{pyo3}::PyObject) {
                   self.context = Some(context);
                }
                /// Register a request middleware function that will be run inside a Tower layer, without cloning the body.
                ##[pyo3(text_signature = "(${'$'}self, func)")]
                pub fn request_middleware(&mut self, py: #{pyo3}::Python, func: #{pyo3}::PyObject) -> #{pyo3}::PyResult<()> {
                    use #{SmithyPython}::PyApp;
                    self.register_middleware(py, func, #{SmithyPython}::PyMiddlewareType::Request)
                }
                /// Main entrypoint: start the server on multiple workers.
                ##[pyo3(text_signature = "(${'$'}self, address, port, backlog, workers)")]
                pub fn run(
                    &mut self,
                    py: #{pyo3}::Python,
                    address: Option<String>,
                    port: Option<i32>,
                    backlog: Option<i32>,
                    workers: Option<usize>,
                ) -> #{pyo3}::PyResult<()> {
                    use #{SmithyPython}::PyApp;
                    self.run_server(py, address, port, backlog, workers)
                }
                /// Lambda entrypoint: start the server on Lambda.
                ##[pyo3(text_signature = "(${'$'}self)")]
                pub fn run_lambda(
                    &mut self,
                    py: #{pyo3}::Python,
                ) -> #{pyo3}::PyResult<()> {
                    use #{SmithyPython}::PyApp;
                    self.run_lambda_handler(py)
                }
                /// Build the router and start a single worker.
                ##[pyo3(text_signature = "(${'$'}self, socket, worker_number)")]
                pub fn start_worker(
                    &mut self,
                    py: pyo3::Python,
                    socket: &pyo3::PyCell<#{SmithyPython}::PySocket>,
                    worker_number: isize,
                ) -> pyo3::PyResult<()> {
                    use #{SmithyPython}::PyApp;
                    let event_loop = self.configure_python_event_loop(py)?;
                    let router = self.build_and_configure_router(py, event_loop)?;
                    self.start_hyper_worker(py, socket, event_loop, router, worker_number)
                }
                """,
                *codegenScope,
            )
            operations.map { operation ->
                val operationName = symbolProvider.toSymbol(operation).name
                val name = operationName.toSnakeCase()
                rustTemplate(
                    """
                    /// Method to register `$name` Python implementation inside the handlers map.
                    /// It can be used as a function decorator in Python.
                    ##[pyo3(text_signature = "(${'$'}self, func)")]
                    pub fn $name(&mut self, py: #{pyo3}::Python, func: #{pyo3}::PyObject) -> #{pyo3}::PyResult<()> {
                        use #{SmithyPython}::PyApp;
                        self.register_operation(py, "$name", func)
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
            /// from $libName import ${Inputs.namespace}
            /// from $libName import ${Outputs.namespace}
            """.trimIndent(),
        )
        if (operations.any { it.errors.isNotEmpty() }) {
            writer.rust("""/// from $libName import ${Errors.namespace}""".trimIndent())
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
        val inputT = "${Inputs.namespace}::${inputSymbol.name}"
        val outputT = "${Outputs.namespace}::${outputSymbol.name}"
        val operationName = symbolProvider.toSymbol(this).name.toSnakeCase()
        return "@app.$operationName\n/// def $operationName(input: $inputT, ctx: Context) -> $outputT"
    }
}
