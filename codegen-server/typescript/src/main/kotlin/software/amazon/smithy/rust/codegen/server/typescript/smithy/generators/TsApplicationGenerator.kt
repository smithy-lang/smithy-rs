/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.typescript.smithy.generators

import software.amazon.smithy.model.shapes.OperationShape
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
        )

    fun render(writer: RustWriter) {
        writer.write("use napi_derive::napi;")
        renderHandlers(writer)
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
                val error = "${operationName}Error"
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
                        $input, #{napi}::threadsafe_function::ErrorStrategy::Fatal
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
}
