/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.python.smithy.generators

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.server.python.smithy.PythonServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.util.toSnakeCase

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
 zz*
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
    private val operations: List<OperationShape>,
) {
    private val symbolProvider = codegenContext.symbolProvider
    private val runtimeConfig = codegenContext.runtimeConfig
    private val codegenScope =
        arrayOf(
            "SmithyPython" to PythonServerCargoDependency.SmithyHttpServerPython(runtimeConfig).asType(),
            "SmithyServer" to ServerCargoDependency.SmithyHttpServer(runtimeConfig).asType(),
            "pyo3" to PythonServerCargoDependency.PyO3.asType(),
            "pyo3asyncio" to PythonServerCargoDependency.PyO3Asyncio.asType(),
            "tokio" to PythonServerCargoDependency.Tokio.asType(),
            "tracing" to PythonServerCargoDependency.Tracing.asType()
        )

    fun render(writer: RustWriter) {
        writer.rustTemplate(
            """
            ##[#{pyo3}::pyclass]
            ##[derive(Debug, Clone)]
            pub struct App {
                handlers: #{SmithyPython}::PyHandlers,
                context: Option<std::sync::Arc<#{pyo3}::PyObject>>,
                locals: #{pyo3asyncio}::TaskLocals,
            }
            """,
            *codegenScope
        )

        renderPyServerTraitImpl(writer)
        renderPyMethods(writer)
    }

    private fun renderPyServerTraitImpl(writer: RustWriter) {
        writer.rustBlockTemplate(
            """
            /// Implement `#{SmithyPython}::PyServer` trait to manage context, handlers and locals.
            impl #{SmithyPython}::PyServer for App
            """,
            *codegenScope
        ) {
            rustTemplate(
                """
                fn handlers_mut(&mut self) -> &mut #{SmithyPython}::PyHandlers {
                    &mut self.handlers as _
                }
                fn locals(&mut self, locals: #{pyo3asyncio}::TaskLocals) {
                    self.locals = locals
                }
                fn context(&self, py: #{pyo3}::Python) -> std::sync::Arc<#{pyo3}::PyObject> {
                    self.context.clone().unwrap_or_else(|| std::sync::Arc::new(py.None()))
                }
                fn start_single_python_worker(&self, py: #{pyo3}::Python) -> #{pyo3}::PyResult<#{pyo3}::PyObject> {
                    use #{pyo3}::IntoPy;
                    self.clone().into_py(py).getattr(py, "start_single_worker")
                }
                """,
                *codegenScope
            )
            rustBlockTemplate("""fn app(&self) -> #{SmithyServer}::Router""", *codegenScope) {
                rustTemplate(
                    """
                    let app = crate::operation_registry::OperationRegistryBuilder::default();
                    """,
                    *codegenScope
                )
                operations.map { operation ->
                    val operationName = symbolProvider.toSymbol(operation).name
                    val name = operationName.toSnakeCase()
                    rustTemplate(
                        """
                        let handlers = self.handlers.clone();
                        let locals = self.locals.clone();
                        let app = app.$name(move |input, state| {
                            let handler = handlers.get("$name").unwrap().clone();
                            #{pyo3asyncio}::tokio::scope(locals.clone(), crate::operation_handler::$name(input, state, handler))
                        });
                        """,
                        *codegenScope
                    )
                }
                write("""app.build().expect("Unable to build operation registry").into()""")
            }
        }
    }

    private fun renderPyMethods(writer: RustWriter) {
        writer.rustBlockTemplate(
            """
            ##[#{pyo3}::pymethods]
            impl App
            """,
            *codegenScope
        ) {
            rustTemplate(
                """
                ##[new]
                pub fn new(py: #{pyo3}::Python) -> #{pyo3}::PyResult<Self> {
                    let asyncio = py.import("asyncio")?;
                    let event_loop = asyncio.call_method0("get_event_loop")?;
                    let locals = #{pyo3asyncio}::TaskLocals::new(event_loop);
                    Ok(Self {
                        handlers: #{SmithyPython}::PyHandlers::new(),
                        context: None,
                        locals,
                    })
                }
                pub fn run(
                    &mut self,
                    py: #{pyo3}::Python,
                    address: Option<String>,
                    port: Option<i32>,
                    backlog: Option<i32>,
                    workers: Option<usize>
                ) -> #{pyo3}::PyResult<()> {
                    use #{SmithyPython}::PyServer;
                    self.start_server(py, address, port, backlog, workers)
                }

                pub fn context(&mut self, _py: #{pyo3}::Python, context: #{pyo3}::PyObject) {
                    self.context = Some(std::sync::Arc::new(context));
                }
                """,
                *codegenScope
            )
            operations.map { operation ->
                val operationName = symbolProvider.toSymbol(operation).name
                val name = operationName.toSnakeCase()
                rustTemplate(
                    """
                    pub fn $name(&mut self, py: #{pyo3}::Python, func: #{pyo3}::PyObject) -> #{pyo3}::PyResult<()> {
                        use #{SmithyPython}::PyServer;
                        self.add_operation(py, "$name", func)
                    }
                    """,
                    *codegenScope
                )
            }
        }
    }
}
