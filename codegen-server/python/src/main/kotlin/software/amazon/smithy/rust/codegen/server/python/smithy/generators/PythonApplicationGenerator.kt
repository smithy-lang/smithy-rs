/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.python.smithy.generators

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.traits.DocumentationTrait
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.server.python.smithy.PythonServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.smithy.CoreCodegenContext
import software.amazon.smithy.rust.codegen.smithy.Errors
import software.amazon.smithy.rust.codegen.smithy.Inputs
import software.amazon.smithy.rust.codegen.smithy.Outputs
import software.amazon.smithy.rust.codegen.util.getTrait
import software.amazon.smithy.rust.codegen.util.inputShape
import software.amazon.smithy.rust.codegen.util.outputShape
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
    coreCodegenContext: CoreCodegenContext,
    private val operations: List<OperationShape>,
) {
    private val crateName = coreCodegenContext.settings.moduleName
    private val symbolProvider = coreCodegenContext.symbolProvider
    private val runtimeConfig = coreCodegenContext.runtimeConfig
    private val model = coreCodegenContext.model
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
        )

    fun render(writer: RustWriter) {
        renderPyApplicationRustDocs(writer)
        writer.rustTemplate(
            """
            ##[#{pyo3}::pyclass]
            ##[derive(Debug, Clone)]
            pub struct App {
                inner: #{SmithyPython}::PyApp
            }
            """,
            *codegenScope
        )

        renderPyMethods(writer)
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
                /// Create a new [App].
                ##[new]
                pub fn new(py: #{pyo3}::Python, log_level: Option<#{SmithyPython}::LogLevel>) -> #{pyo3}::PyResult<Self> {
                    let log_level = log_level.unwrap_or(#{SmithyPython}::LogLevel::Info);
                    #{SmithyPython}::logging::setup(py, log_level)?;
                    Ok(Self { inner: aws_smithy_http_server_python::PyApp::default() })
                }
                /// Register a context object that will be shared between handlers.
                pub fn context(&mut self, py: #{pyo3}::Python, context: #{pyo3}::PyObject) {
                    self.inner.context(py, context)
                }
                /// Run the Python application.
                pub fn run(
                    &mut self,
                    py: #{pyo3}::Python,
                    address: Option<String>,
                    port: Option<i32>,
                    backlog: Option<i32>,
                    workers: Option<usize>,
                ) -> #{pyo3}::PyResult<()> {
                    self.build_router(py)?;
                    self.inner.run(py, address, port, backlog, workers)
                }
                """,
                *codegenScope
            )
            rustBlockTemplate(
                """
                /// Dynamically codegenerate the routes, allowing to build the Smithy [#{SmithyServer}::Router].
                pub fn build_router(&mut self, py: #{pyo3}::Python) -> #{pyo3}::PyResult<()>
                """,
                *codegenScope
            ) {
                rustTemplate(
                    """
                    let asyncio = py.import("asyncio")?;
                    let event_loop = asyncio.call_method0("get_event_loop")?;
                    let router = crate::operation_registry::OperationRegistryBuilder::default();
                    """,
                    *codegenScope
                )
                for (operation in operations) {
                    val operationName = symbolProvider.toSymbol(operation).name
                    val name = operationName.toSnakeCase()
                    rustTemplate(
                        """
                        let ${name}_locals = pyo3_asyncio::TaskLocals::new(event_loop);
                        let handler = self.inner.handlers.get("$name").expect("Python handler for operation `$name` not found").clone();
                        let router = router.$name(move |input, state| {
                            #{pyo3_asyncio}::tokio::scope(${name}_locals, crate::operation_handler::$name(input, state, handler))
                        });
                        """,
                        *codegenScope
                    )
                }
                rustTemplate(
                    """
                    let router: #{SmithyServer}::Router = router.build().expect("Unable to build operation registry").into();
                    self.inner.router = Some(#{SmithyPython}::PyRouter(router));
                    Ok(())
                    """,
                    *codegenScope
                )
            }
            operations.map { operation ->
                val operationName = symbolProvider.toSymbol(operation).name
                val name = operationName.toSnakeCase()
                rustTemplate(
                    """
                    /// Method to register `$name` Python implementation inside the handlers map.
                    /// It can be used as a function decorator in Python.
                    pub fn $name(&mut self, py: #{pyo3}::Python, func: #{pyo3}::PyObject) -> #{pyo3}::PyResult<()> {
                        self.inner.register_operation(py, "$name", func)
                    }
                    """,
                    *codegenScope
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
/// Operations can be registrered using the application object as a decorator (`@app.operation_name`).
///
/// Here's a full example to get you started using coroutines:
///
/// ```python
${ if (operations.any { it.errors.isNotEmpty() }) {
"""/// from $crateName import ${Inputs.namespace}
/// from $crateName import ${Outputs.namespace}
/// from $crateName import ${Errors.namespace}"""
            } else {
"""/// from $crateName import ${Inputs.namespace}
/// from $crateName import ${Outputs.namespace}"""
            } }
/// from $crateName import App
///
/// @dataclass
/// class Context:
///     counter: int = 0
///
/// app = App()
/// app.context(Context())
///
${operationImplementationStubs(operations)}
///
/// app.run()
/// ```
///
/// Any of operations above can be written as well prepending the `async` keyword and
/// the Python application will automatically handle it and schedule it on the even loop for you.
            """
        )
    }

    private fun operationImplementationStubs(operations: List<OperationShape>): String =
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
        }

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
