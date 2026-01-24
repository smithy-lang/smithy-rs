/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.typescript.smithy.generators

import software.amazon.smithy.model.knowledge.TopDownIndex
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute.Companion.derive
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.util.toPascalCase
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerProtocol
import software.amazon.smithy.rust.codegen.server.typescript.smithy.TsServerCargoDependency

/**
 * Generates a Typescript compatible application and server that can be configured from Typescript.
 */
class TsApplicationGenerator(
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
            "SmithyServer" to ServerCargoDependency.smithyHttpServer(runtimeConfig).toType(),
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
            "http" to RuntimeType.Http0x,
            "socket2" to TsServerCargoDependency.Socket2.toType(),
        )

    fun render(writer: RustWriter) {
        writer.write("use napi_derive::napi;")
        renderHandlers(writer)
        renderApp(writer)

        // TODO(https://github.com/napi-rs/napi-rs/issues/1377) Move these to be part of the runtime crate.
        renderSocket(writer)
        renderServer(writer)
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
                        #{napi}::threadsafe_function::ErrorStrategy::Fatal
                    >,
                    """,
                    *codegenScope,
                )
            }
        }
        Attribute("""napi(object)""").render(writer)
        writer.rustBlock("pub struct TsHandlers") {
            operations.map { operation ->
                val operationName = symbolProvider.toSymbol(operation).name
                val input = "${operationName}Input"
                val output = "${operationName}Output"
                val fnName = operationName.toSnakeCase()
                rustTemplate(
                    """
                    ##[napi(ts_type = "(input: $input) => Promise<$output>")]
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
            """pub fn create(ts_handlers: TsHandlers) -> #{napi}::Result<Self>""",
            *codegenScope,
        ) {
            operations.map { operation ->
                val operationName = symbolProvider.toSymbol(operation).name
                val input = "crate::input::${operationName}Input"
                val fnName = operationName.toSnakeCase()
                rustTemplate(
                    """
                    let $fnName: #{napi}::threadsafe_function::ThreadsafeFunction<
                        $input, #{napi}::threadsafe_function::ErrorStrategy::Fatal
                    > = ts_handlers.$fnName.create_threadsafe_function(0, |ctx| Ok(vec![ctx.value]))?;
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
            """pub fn start(&self, socket: &TsSocket) -> #{napi}::Result<()>""",
            *codegenScope,
        ) {
            rustTemplate(
                """
                let plugins = #{SmithyServer}::plugin::PluginPipeline::new();
                let builder = crate::service::$serviceName::builder_with_plugins(plugins);
                """,
                *codegenScope,
            )
            operations.map { operation ->
                val operationName = symbolProvider.toSymbol(operation).name.toSnakeCase()
                rust("let builder = builder.$operationName(crate::ts_operation_adaptor::$operationName);")
            }
            rustTemplate(
                """
                let app = builder.build().expect("failed to build instance of $serviceName")
                    .layer(&#{SmithyServer}::AddExtensionLayer::new(self.handlers.clone()));
                let service = #{tower}::util::BoxCloneService::new(app);
                start_hyper_worker(socket, service).expect("failed to start the hyper server");
                Ok(())
                """,
                *codegenScope,
            )
        }
    }

    private fun renderSocket(writer: RustWriter) {
        Attribute("napi").render(writer)
        Attribute(derive(RuntimeType.Debug)).render(writer)
        writer.rustTemplate(
            """
            pub struct TsSocket(#{socket2}::Socket);
            """,
            *codegenScope,
        )

        Attribute("napi").render(writer)
        writer.rustBlock("impl TsSocket") {
            writer.rust(
                """
                /// Create a new UNIX `Socket` from an address, port and backlog.
                /// If not specified, the backlog defaults to 1024 connections.
                """.trimIndent(),
            )
            Attribute("napi(constructor)").render(writer)
            writer.rustBlockTemplate(
                """pub fn new(address: String, port: i32, backlog: Option<i32>) -> #{napi}::Result<Self>""".trimIndent(),
                *codegenScope,
            ) {
                writer.rustTemplate(
                    """
                    let socket = Self::new_socket(address, port, backlog)
                        .map_err(|e| #{napi}::Error::from_reason(e.to_string()))?;
                    Ok(Self(socket))
                    """,
                    *codegenScope,
                )
            }
            writer.rustBlockTemplate(
                """pub fn new_socket(address: String, port: i32, backlog: Option<i32>) -> Result<#{socket2}::Socket, Box<dyn std::error::Error>> """.trimIndent(),
                *codegenScope,
            ) {
                writer.rustTemplate(
                    """
                    let address: std::net::SocketAddr = format!("{}:{}", address, port).parse()?;
                    let domain = if address.is_ipv6() {
                        #{socket2}::Domain::IPV6
                    } else {
                        #{socket2}::Domain::IPV4
                    };
                    let socket = #{socket2}::Socket::new(domain, #{socket2}::Type::STREAM, Some(#{socket2}::Protocol::TCP))?;
                    // Set value for the `SO_REUSEPORT` and `SO_REUSEADDR` options on this socket.
                    // This indicates that further calls to `bind` may allow reuse of local
                    // addresses. For IPv4 sockets this means that a socket may bind even when
                    // there's a socket already listening on this port.
                    socket.set_reuse_port(true)?;
                    socket.set_reuse_address(true)?;
                    socket.bind(&address.into())?;
                    socket.listen(backlog.unwrap_or(1024))?;
                    Ok(socket)
                    """.trimIndent(),
                    *codegenScope,
                )
            }

            writer.rust(
                """
                /// Clone the inner socket allowing it to be shared between multiple
                /// Nodejs processes.
                """.trimIndent(),
            )
            Attribute("napi").render(writer)
            writer.rustBlockTemplate(
                """pub fn try_clone(&self) -> #{napi}::Result<Self>""".trimIndent(),
                *codegenScope,
            ) {
                writer.rustTemplate(
                    """
                    Ok(TsSocket(
                        self.0
                            .try_clone()
                            .map_err(|e| #{napi}::Error::from_reason(e.to_string()))?,
                    ))
                    """.trimIndent(),
                    *codegenScope,
                )
            }
        }

        writer.rustBlock("impl TsSocket") {
            writer.rustTemplate(
                """
                pub fn to_raw_socket(&self) -> #{napi}::Result<#{socket2}::Socket> {
                self.0
                 .try_clone()
                 .map_err(|e| #{napi}::Error::from_reason(e.to_string()))
                }

                """.trimIndent(),
                *codegenScope,
            )
        }
    }

    private fun renderServer(writer: RustWriter) {
        writer.rustBlockTemplate(
            """
            pub fn start_hyper_worker(
            socket: &TsSocket,
            app: #{tower}::util::BoxCloneService<
                #{http}::Request<#{SmithyServer}::body::Body>,
                #{http}::Response<#{SmithyServer}::body::BoxBody>,
                std::convert::Infallible,
            >,
                ) -> #{napi}::Result<()>
            """.trimIndent(),
            *codegenScope,
        ) {
            writer.rustTemplate(
                """
                let server = #{hyper}::Server::from_tcp(
                        socket
                            .to_raw_socket()?
                            .try_into()
                            .expect("Unable to convert socket2::Socket into std::net::TcpListener"),
                )
                .expect("Unable to create hyper server from shared socket")
                .serve(#{SmithyServer}::routing::IntoMakeService::new(app));

                let handle = #{tokio}::runtime::Handle::current();
                handle.spawn(async move {
                    // Process each socket concurrently.
                    server.await
                });

                Ok(())
                """.trimIndent(),
                *codegenScope,
            )
        }
    }
}
