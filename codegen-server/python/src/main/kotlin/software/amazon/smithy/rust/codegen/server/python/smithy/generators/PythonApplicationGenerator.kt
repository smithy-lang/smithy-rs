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
            "tracing" to PythonServerCargoDependency.Tracing.asType(),
            "tower" to PythonServerCargoDependency.Tower.asType(),
            "towerhttp" to PythonServerCargoDependency.TowerHttp.asType(),
            "numcpus" to PythonServerCargoDependency.NumCpus.asType(),
            "hyper" to PythonServerCargoDependency.Hyper.asType()
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

        renderApplication(writer)
        renderPyMethods(writer)
    }

    private fun renderApplication(writer: RustWriter) {
        writer.rustBlockTemplate(
            """
            /// Implement [App] to manage context, handlers and locals.
            impl App
            """,
            *codegenScope
        ) {
            rustTemplate(
                """
                ${renderAddOperation()}
                ${renderStartSingleWorker()}
                ${renderStartServer()}

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

    // Register a new operation in the handlers map.
    private fun renderAddOperation(): String =
        """
        fn add_operation(&mut self, py: #{pyo3}::Python, name: &str, func: #{pyo3}::PyObject) -> #{pyo3}::PyResult<()> {
            let inspect = py.import("inspect")?;
            // Check if the function is a coroutine.
            // NOTE: that `asyncio.iscoroutine()` doesn't work here.
            let is_coroutine = inspect
                .call_method1("iscoroutinefunction", (&func,))?
                .extract::<bool>()?;
            // Find number of expected methods (a Python implementation could not accept the context).
            let func_args = inspect
                .call_method1("getargs", (func.getattr(py, "__code__")?,))?
                .getattr("args")?
                .extract::<Vec<String>>()?;
            let func = #{SmithyPython}::PyHandler {
                func,
                is_coroutine,
                args: func_args.len(),
            };
            tracing::info!(
                "Registering {} function `{}` for operation {} with {} arguments",
                if func.is_coroutine { "async" } else { "sync" },
                name,
                func.func,
                func.args
            );
            // Insert the handler in the handlers map.
            self.handlers
                .insert(String::from(name), std::sync::Arc::new(func));
            Ok(())
        }

        """

    /**
     * Start a signle worker with its own Tokio and Python async runtime
     * and using the provided shared socket.
     */
    private fun renderStartSingleWorker(): String =
        """
        ##[allow(dead_code)]
        fn start_single_worker(
            &'static mut self,
            py: #{pyo3}::Python,
            socket: &#{pyo3}::PyCell<#{SmithyPython}::SharedSocket>,
            worker_number: isize,
        ) -> #{pyo3}::PyResult<()> {
            tracing::info!("Starting Rust Python server worker {}", worker_number);
            // Clone the socket.
            let borrow = socket.try_borrow_mut()?;
            let held_socket: &#{SmithyPython}::SharedSocket = &*borrow;
            let raw_socket = held_socket.get_socket()?;
            // Setup the Python asyncio loop to use `uvloop`.
            let asyncio = py.import("asyncio")?;
            let uvloop = py.import("uvloop")?;
            uvloop.call_method0("install")?;
            tracing::debug!("Setting up uvloop for current process");
            let event_loop = asyncio.call_method0("new_event_loop")?;
            asyncio.call_method1("set_event_loop", (event_loop,))?;
            // Create `State` object from the Python context object.
            let context = self.context.clone().unwrap_or_else(|| std::sync::Arc::new(py.None()));
            let state = #{SmithyPython}::State::new(context);

            tracing::debug!("Start the Tokio runtime in a background task");
            // Store Python event loop locals.
            self.locals = pyo3_asyncio::TaskLocals::new(event_loop);
            // Spawn a new background [std::thread] to run the application.
            std::thread::spawn(move || {
                // The thread needs a new [tokio] runtime.
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .thread_name(format!("smithy-rs[{}]", worker_number))
                    .build()
                    .unwrap();
                // Register operations into a Router, add middleware and start the `hyper` server,
                // all inside a [tokio] blocking function.
                rt.block_on(async move {
                    tracing::debug!("Add middlewares to Rust Python router");
                    let app = self.app().layer(
                        #{tower}::ServiceBuilder::new()
                            .layer(#{towerhttp}::trace::TraceLayer::new_for_http())
                            .layer(#{SmithyServer}::AddExtensionLayer::new(state)),
                    );
                    tracing::debug!("Starting hyper server from shared socket");
                    let server = #{hyper}::Server::from_tcp(raw_socket.try_into().unwrap())
                        .unwrap()
                        .serve(app.into_make_service());

                    // Run forever-ish...
                    if let Err(err) = server.await {
                        tracing::error!("server error: {}", err);
                    }
                });
            });
            // Block on the event loop forever.
            tracing::debug!("Run and block on the Python event loop");
            let event_loop = (*event_loop).call_method0("run_forever");
            tracing::info!("Rust Python server started successfully");
            if event_loop.is_err() {
                tracing::warn!("Ctrl-c handler, quitting");
            }
            Ok(())
        }
        """
    /**
     * Start the server on multiple workers.
     *
     * The multiprocessing server is achieved using the ability of a Python interpreter
     * to clone and start itself as a new process.
     * The shared sockets is created and Using the [multiprocessing::Process] module, multiple
     * workers with the method `self.start_single_python_worker()` as target are started.
     *
     * [multiprocessing::Process]: https://docs.python.org/3/library/multiprocessing.html
     */
    private fun renderStartServer(): String =
        """
        fn start_server(
            &mut self,
            py: #{pyo3}::Python,
            address: Option<String>,
            port: Option<i32>,
            backlog: Option<i32>,
            workers: Option<usize>,
        ) -> #{pyo3}::PyResult<()> {
            let mp = py.import("multiprocessing")?;
            mp.call_method0("allow_connection_pickling")?;
            let address = address.unwrap_or_else(|| String::from("127.0.0.1"));
            let port = port.unwrap_or(8080);
            let socket = #{SmithyPython}::SharedSocket::new(address, port, backlog)?;
            use pyo3::IntoPy;
            for idx in 0..workers.unwrap_or_else(#{numcpus}::get) {
                let sock = socket.try_clone()?;
                let process = mp.getattr("Process")?;
                let handle = process.call1((
                    py.None(),
                    self.clone().into_py(py).getattr(py, "start_single_worker")?,
                    format!("smithy-rs[{}]", idx),
                    (sock.into_py(py), idx),
                ))?;
                handle.call_method0("start")?;
            }
            Ok(())
        }
        """

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
                        self.add_operation(py, "$name", func)
                    }
                    """,
                    *codegenScope
                )
            }
        }
    }
}
