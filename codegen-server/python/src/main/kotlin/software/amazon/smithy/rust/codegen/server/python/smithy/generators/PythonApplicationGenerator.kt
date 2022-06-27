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
import software.amazon.smithy.rust.codegen.smithy.CoreCodegenContext
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
    private val symbolProvider = coreCodegenContext.symbolProvider
    private val runtimeConfig = coreCodegenContext.runtimeConfig
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
            "hyper" to PythonServerCargoDependency.Hyper.asType()
        )

    fun render(writer: RustWriter) {
        writer.rustTemplate(
            """
            ##[#{pyo3}::pyclass(extends = #{SmithyPython}::PyApp)]
            ##[derive(Debug, Clone)]
            pub struct App { }
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
            rustBlockTemplate(
                """
                /// Override the `router()` function of #{SmithyPython}::PyApp allowing to dynamically
                /// codegenerate the routes.
                pub fn router(self_: #{pyo3}::PyRef<'_, Self>) -> Option<#{pyo3}::PyObject>
                """,
                *codegenScope
            ) {
                rustTemplate(
                    """
                    let router = crate::operation_registry::OperationRegistryBuilder::default();
                    let sup = self_.as_ref();
                    """,
                    *codegenScope
                )
                for (operation in operations) {
                    val operationName = symbolProvider.toSymbol(operation).name
                    val name = operationName.toSnakeCase()
                    rustTemplate(
                        """
                        let locals = sup.locals.clone();
                        let handler = sup.handlers.get("$name").expect("Python handler for `{$name}` not found").clone();
                        let router = router.$name(move |input, state| {
                            #{pyo3_asyncio}::tokio::scope(locals.clone(), crate::operation_handler::$name(input, state, handler))
                        });
                        """,
                        *codegenScope
                    )
                }
                rustTemplate(
                    """
                    let router: #{SmithyServer}::Router = router.build().expect("Unable to build operation registry").into();
                    use #{pyo3}::IntoPy;
                    Some(#{SmithyPython}::PyRouter(router).into_py(self_.py()))
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
                    pub fn $name(self_: #{pyo3}::PyRefMut<'_, Self>, func: #{pyo3}::PyObject) -> #{pyo3}::PyResult<()> {
                        let mut sup = self_.into_super();
                        #{pyo3}::Python::with_gil(|py| sup.register_operation(py, "$name", func))
                    }
                    """,
                    *codegenScope
                )
            }
        }
    }
}
