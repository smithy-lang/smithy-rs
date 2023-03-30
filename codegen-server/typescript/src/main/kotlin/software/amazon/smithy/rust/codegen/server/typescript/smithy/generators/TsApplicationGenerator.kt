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
    private val operations = index.getContainedOperations(codegenContext.serviceShape).toSortedSet(
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
            "SmithyTs" to TsServerCargoDependency.smithyHttpServerTs(runtimeConfig).toType(),
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
            "http" to RuntimeType.Http,
            "socket2" to TsServerCargoDependency.Socket2.toType(),
        )

    fun render(writer: RustWriter) {
        writer.write("use napi_derive::napi;")
        renderHandlers(writer)
        renderApp(writer)

        // TODO Move these to be runtime once "#1377: Re-export napi symbols from dependencies
        //  (https://github.com/napi-rs/napi-rs/issues/1377) is solved.
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
                // #{SmithyTs}::setup_tracing();
                let plugins = #{SmithyServer}::plugin::PluginPipeline::new();
                let builder = crate::service::PokemonService::builder_with_plugins(plugins);
                """,
                *codegenScope,
            )
            operations.map { operation ->
                val operationName = symbolProvider.toSymbol(operation).name.toSnakeCase()
                rust("let builder = builder.$operationName(crate::ts_operation_adaptor::$operationName);")
            }
            rustTemplate(
                """
                let app = builder.build().expect("failed to build instance of PokemonService")
                    .layer(&#{SmithyServer}::AddExtensionLayer::new(self.handlers.clone()));
                let service = #{tower}::util::BoxCloneService::new(app);
                // #{SmithyTs}::server::start_hyper_worker(socket, service).expect("failed to start the hyper server");
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
                    // XXX How do I make the new_socket import here?
                    // In the previous file, it was use aws_smithy_http_server::socket::new_socket;
                    """
                    Ok(Self(
                        aws_smithy_http_server::socket::new_socket(address, port, backlog)
                        .map_err(|e| #{napi}::Error::from_reason(e.to_string()))?,
                    ))
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
                // XXX Can I change the signature to #{napi}::Result<Self>?
                """pub fn try_clone(&self) -> #{napi}::Result<TsSocket>""".trimIndent(),
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
            writer.rustBlockTemplate(
                """pub fn to_raw_socket(&self) -> #{napi}::Result<#{socket2}::Socket>""".trimIndent(),
                *codegenScope,
            ) {
                writer.rustTemplate(
                    """
                    self.0
                     .try_clone()
                     .map_err(|e| #{napi}::Error::from_reason(e.to_string()))
                    """.trimIndent(),
                    *codegenScope,
                )
            }
        }
    }

    private fun renderServer(writer: RustWriter) {
        writer.rustBlockTemplate(
            // / XXX Check here the aws_smithy_http_server (same issue with new_socket above)
            """pub fn start_hyper_worker(
            socket: &TsSocket,
            app: #{tower}::util::BoxCloneService<
                #{http}::Request<aws_smithy_http_server::body::Body>,
                #{http}::Response<aws_smithy_http_server::body::BoxBody>,
                std::convert::Infallible,
            >,
                ) -> #{napi}::Result<()>
            """.trimIndent(),
            *codegenScope,
        ) {
            // XXX Check here IntoMakeService
            writer.rustTemplate(
                """
                let server = #{hyper}::Server::from_tcp(
                        socket
                            .to_raw_socket()?
                            .try_into()
                            .expect("Unable to convert socket2::Socket into std::net::TcpListener"),
                )
                .expect("Unable to create hyper server from shared socket")
                .serve(aws_smithy_http_server::routing::IntoMakeService::new(app));

                // TODO(https://github.com/awslabs/smithy-rs/issues/2317) Find a better, non-blocking way
                // to spawn the server.
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
