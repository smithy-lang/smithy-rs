/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.server.smithy

import java.util.logging.Logger
import software.amazon.smithy.build.PluginContext
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.neighbor.Walker
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeVisitor
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.rustlang.RustMetadata
import software.amazon.smithy.rust.codegen.rustlang.RustModule
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServiceGenerator
import software.amazon.smithy.rust.codegen.server.smithy.protocols.RestJson1HttpDeserializerGenerator
import software.amazon.smithy.rust.codegen.server.smithy.protocols.RestJson1HttpSerializerGenerator
import software.amazon.smithy.rust.codegen.server.smithy.protocols.ServerGenerator
import software.amazon.smithy.rust.codegen.smithy.protocols.serialize.JsonSerializerGenerator
import software.amazon.smithy.rust.codegen.smithy.protocols.parse.JsonParserGenerator
import software.amazon.smithy.rust.codegen.smithy.DefaultPublicModules
import software.amazon.smithy.rust.codegen.smithy.RustCrate
import software.amazon.smithy.rust.codegen.smithy.RustSettings
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.SymbolVisitorConfig
import software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.smithy.generators.BuilderGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.HttpProtocolGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolConfig
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolGeneratorFactory
import software.amazon.smithy.rust.codegen.smithy.generators.StructureGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.implBlock
import software.amazon.smithy.rust.codegen.smithy.letIf
import software.amazon.smithy.rust.codegen.smithy.protocols.HttpTraitHttpBindingResolver
import software.amazon.smithy.rust.codegen.smithy.protocols.ProtocolContentTypes
import software.amazon.smithy.rust.codegen.smithy.protocols.ProtocolLoader
import software.amazon.smithy.rust.codegen.smithy.protocols.HttpBindingResolver
import software.amazon.smithy.rust.codegen.smithy.traits.SyntheticInputTrait
import software.amazon.smithy.rust.codegen.smithy.transformers.AddErrorMessage
import software.amazon.smithy.rust.codegen.smithy.transformers.OperationNormalizer
import software.amazon.smithy.rust.codegen.smithy.transformers.RecursiveShapeBoxer
import software.amazon.smithy.rust.codegen.smithy.transformers.RemoveEventStreamOperations
import software.amazon.smithy.rust.codegen.util.CommandFailed
import software.amazon.smithy.rust.codegen.util.hasTrait
import software.amazon.smithy.rust.codegen.smithy.protocols.HttpLocation
import software.amazon.smithy.rust.codegen.util.runCommand
import software.amazon.smithy.rust.codegen.util.inputShape
import software.amazon.smithy.rust.codegen.util.outputShape

class CodegenVisitor(context: PluginContext, private val codegenDecorator: RustCodegenDecorator) :
        ShapeVisitor.Default<Unit>() {

    private val logger = Logger.getLogger(javaClass.name)
    private val settings = RustSettings.from(context.model, context.settings)

    private val symbolProvider: RustSymbolProvider
    private val rustCrate: RustCrate
    private val fileManifest = context.fileManifest
    private val model: Model
    private val protocolConfig: ProtocolConfig
    private val protocolGenerator: ProtocolGeneratorFactory<HttpProtocolGenerator>
    private val httpGenerator: HttpProtocolGenerator

    private val serializerGenerator: JsonSerializerGenerator
    private val deserializerGenerator: JsonParserGenerator
    private val httpSerializerGenerator: ServerGenerator
    private val httpDeserializerGenerator: ServerGenerator
    private val httpBindingResolver: HttpBindingResolver

    init {
        val symbolVisitorConfig =
                SymbolVisitorConfig(
                        runtimeConfig = settings.runtimeConfig,
                        codegenConfig = settings.codegenConfig
                )
        val baseModel = baselineTransform(context.model)
        val service = settings.getService(baseModel)
        val (protocol, generator) =
                ProtocolLoader(
                                codegenDecorator.protocols(
                                        service.id,
                                        ProtocolLoader.DefaultProtocols
                                )
                        )
                        .protocolFor(context.model, service)
        protocolGenerator = generator
        model = generator.transformModel(codegenDecorator.transformModel(service, baseModel))
        val baseProvider = RustCodegenPlugin.baseSymbolProvider(model, service, symbolVisitorConfig)
        symbolProvider =
                codegenDecorator.symbolProvider(generator.symbolProvider(model, baseProvider))

        protocolConfig =
                ProtocolConfig(
                        model,
                        symbolProvider,
                        settings.runtimeConfig,
                        service,
                        protocol,
                        settings.moduleName
                )

        rustCrate = RustCrate(context.fileManifest, symbolProvider, DefaultPublicModules)
        httpGenerator = protocolGenerator.buildProtocolGenerator(protocolConfig)

        httpBindingResolver =
                HttpTraitHttpBindingResolver(
                        protocolConfig.model,
                        ProtocolContentTypes.consistent("application/json"),
                )

        serializerGenerator = JsonSerializerGenerator(protocolConfig, httpBindingResolver)
        deserializerGenerator = JsonParserGenerator(protocolConfig, httpBindingResolver)
        when (protocolConfig.protocol.toString()) {
            "aws.protocols#restJson1" -> {
                httpSerializerGenerator =
                        RestJson1HttpSerializerGenerator(protocolConfig, httpBindingResolver)
                httpDeserializerGenerator =
                        RestJson1HttpDeserializerGenerator(protocolConfig, httpBindingResolver)
            }
            else -> {
                // TODO: support other protocols
                throw Exception("Protocol ${protocolConfig.protocol} not support yet")
            }
        }
    }

    private fun baselineTransform(model: Model) =
            model
                    .let(RecursiveShapeBoxer::transform)
                    .letIf(settings.codegenConfig.addMessageToErrors, AddErrorMessage::transform)
                    .let(OperationNormalizer::transform)
                    .let { RemoveEventStreamOperations.transform(it, settings) }
    // .let(EventStreamNormalizer::transform)

    fun execute() {
        val service = settings.getService(model)
        logger.info(
                "[rust-server-codegen] Generating Rust server for service ${service}, protocol ${protocolConfig.protocol}..."
        )
        val serviceShapes = Walker(model).walkShapes(service)
        serviceShapes.forEach { it.accept(this) }
        codegenDecorator.extras(protocolConfig, rustCrate)
        val module = RustMetadata(public = true)
        rustCrate.withModule(RustModule("error", module)) { writer -> renderSerdeError(writer) }
        rustCrate.finalize(settings, codegenDecorator.libRsCustomizations(protocolConfig, listOf()))
        try {
            "cargo fmt".runCommand(
                    fileManifest.baseDir,
                    timeout = settings.codegenConfig.formatTimeoutSeconds.toLong()
            )
        } catch (err: CommandFailed) {
            logger.warning(
                    "[rust-server-codegen] Failed to run cargo fmt: [${service.id}]\n${err.output}"
            )
        }

        logger.info("[rust-server-codegen] Rust server generation complete!")
    }

    override fun getDefault(shape: Shape?) {}

    override fun operationShape(shape: OperationShape?) {
        logger.info("[rust-server-codegen] Generating operation ${shape}...")
        if (shape != null) {
            val inputHttpDocumentMembers =
                httpBindingResolver.requestMembers(shape, HttpLocation.DOCUMENT)
            val outputHttpDocumentMembers = 
                httpBindingResolver.responseMembers(shape, HttpLocation.DOCUMENT)
            rustCrate.useShapeWriter(shape) { writer ->
                shape.let {
                    httpDeserializerGenerator.render(writer, it)
                    httpSerializerGenerator.render(writer, it)
                    serializerGenerator.renderStructure(writer, shape.inputShape(model), inputHttpDocumentMembers)
                    serializerGenerator.renderStructure(writer, shape.outputShape(model), outputHttpDocumentMembers)
                    deserializerGenerator.renderStructure(writer, shape.inputShape(model), inputHttpDocumentMembers)
                    deserializerGenerator.renderStructure(writer, shape.outputShape(model), outputHttpDocumentMembers)
                    shape.errors.forEach { error ->
                        val errorShape = model.expectShape(error, StructureShape::class.java)
                        serializerGenerator.renderStructure(writer, errorShape, errorShape.members().toList())
                        deserializerGenerator.renderStructure(writer, errorShape, errorShape.members().toList())
                    }
                }
            }
        }
    }

    override fun structureShape(shape: StructureShape) {
        logger.info("[rust-server-codegen] Generating a structure ${shape}...")
        rustCrate.useShapeWriter(shape) { writer ->
            StructureGenerator(model, symbolProvider, writer, shape).render()
            if (!shape.hasTrait<SyntheticInputTrait>()) {
                val builderGenerator =
                        BuilderGenerator(protocolConfig.model, protocolConfig.symbolProvider, shape)
                builderGenerator.render(writer)
                writer.implBlock(shape, symbolProvider) {
                    builderGenerator.renderConvenienceMethod(this)
                }
            }
        }
    }

    override fun serviceShape(shape: ServiceShape) {
        ServiceGenerator(
                        rustCrate,
                        httpGenerator,
                        protocolGenerator.support(),
                        protocolConfig,
                        codegenDecorator
                )
                .render()
    }

    private fun renderSerdeError(writer: RustWriter) {
        writer.rust(
                """
                ##[derive(Debug)]
                pub enum Error {
                    Generic(std::borrow::Cow<'static, str>),
                    DeserializeJson(smithy_json::deserialize::Error),
                    DeserializeHeader(smithy_http::header::ParseError),
                    DeserializeLabel(std::string::String),
                    BuildInput(smithy_http::operation::BuildError),
                    BuildResponse(http::Error),
                    SmithyType(smithy_types::Error),
                }
                
                impl Error {
                    ##[allow(dead_code)]
                    fn generic(msg: &'static str) -> Self {
                        Self::Generic(msg.into())
                    }
                }
                
                impl From<smithy_json::deserialize::Error> for Error {
                    fn from(err: smithy_json::deserialize::Error) -> Self {
                        Self::DeserializeJson(err)
                    }
                }
                
                impl From<smithy_http::header::ParseError> for Error {
                    fn from(err: smithy_http::header::ParseError) -> Self {
                        Self::DeserializeHeader(err)
                    }
                }
                
                impl From<smithy_http::operation::BuildError> for Error {
                    fn from(err: smithy_http::operation::BuildError) -> Self {
                        Self::BuildInput(err)
                    }
                }

                impl From<smithy_types::Error> for Error {
                    fn from(err: smithy_types::Error) -> Self {
                        Self::SmithyType(err)
                    }
                }
                                
                impl std::fmt::Display for Error {
                    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        match *self {
                            Self::Generic(ref msg) => write!(f, "serde error: {}", msg),
                            Self::DeserializeJson(ref err) => write!(f, "json parse error: {}", err),
                            Self::DeserializeHeader(ref err) => write!(f, "header parse error: {}", err),
                            Self::DeserializeLabel(ref msg) => write!(f, "label parse error: {}", msg),
                            Self::BuildInput(ref err) => write!(f, "json payload error: {}", err),
                            Self::BuildResponse(ref err) => write!(f, "http response error: {}", err),
                            Self::SmithyType(ref err) => write!(f, "type error: {}", err),
                        }
                    }
                }
                
                impl std::error::Error for Error {}
            """.trimIndent()
        )
    }
}
