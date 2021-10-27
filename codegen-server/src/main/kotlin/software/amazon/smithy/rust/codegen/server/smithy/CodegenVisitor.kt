/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.aws.traits.protocols.RestJson1Trait
import software.amazon.smithy.build.PluginContext
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.neighbor.Walker
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeVisitor
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.rust.codegen.rustlang.RustMetadata
import software.amazon.smithy.rust.codegen.rustlang.RustModule
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServiceGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ProtocolSupport
import software.amazon.smithy.rust.codegen.server.smithy.protocols.RestJson1HttpDeserializerGenerator
import software.amazon.smithy.rust.codegen.server.smithy.protocols.RestJson1HttpSerializerGenerator
import software.amazon.smithy.rust.codegen.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.smithy.DefaultPublicModules
import software.amazon.smithy.rust.codegen.smithy.RustCrate
import software.amazon.smithy.rust.codegen.smithy.RustSettings
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.SymbolVisitorConfig
import software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.smithy.generators.BuilderGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.EnumGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.StructureGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.UnionGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.implBlock
import software.amazon.smithy.rust.codegen.smithy.generators.protocol.ProtocolGenerator
import software.amazon.smithy.rust.codegen.smithy.letIf
import software.amazon.smithy.rust.codegen.smithy.protocols.HttpBindingResolver
import software.amazon.smithy.rust.codegen.smithy.protocols.HttpTraitHttpBindingResolver
import software.amazon.smithy.rust.codegen.smithy.protocols.ProtocolContentTypes
import software.amazon.smithy.rust.codegen.smithy.protocols.ProtocolGeneratorFactory
import software.amazon.smithy.rust.codegen.smithy.protocols.ProtocolLoader
import software.amazon.smithy.rust.codegen.smithy.traits.SyntheticInputTrait
import software.amazon.smithy.rust.codegen.smithy.transformers.AddErrorMessage
import software.amazon.smithy.rust.codegen.smithy.transformers.EventStreamNormalizer
import software.amazon.smithy.rust.codegen.smithy.transformers.OperationNormalizer
import software.amazon.smithy.rust.codegen.smithy.transformers.RecursiveShapeBoxer
import software.amazon.smithy.rust.codegen.smithy.transformers.RemoveEventStreamOperations
import software.amazon.smithy.rust.codegen.util.CommandFailed
import software.amazon.smithy.rust.codegen.util.getTrait
import software.amazon.smithy.rust.codegen.util.hasTrait
import software.amazon.smithy.rust.codegen.util.runCommand
import java.util.logging.Logger

/**
 * Entrypoint for server-side code generation. This class will walk the in-memory model and
 * generate all the needed types by calling the accept() function on the available shapes.
 */
class CodegenVisitor(context: PluginContext, private val codegenDecorator: RustCodegenDecorator) :
    ShapeVisitor.Default<Unit>() {

    private val logger = Logger.getLogger(javaClass.name)
    private val settings = RustSettings.from(context.model, context.settings)

    private val symbolProvider: RustSymbolProvider
    private val rustCrate: RustCrate
    private val fileManifest = context.fileManifest
    private val model: Model
    private val codegenContext: CodegenContext
    private val protocolGeneratorFactory: ProtocolGeneratorFactory<ProtocolGenerator>
    private val protocolGenerator: ProtocolGenerator

    private val httpSerializerGenerator: RestJson1HttpSerializerGenerator
    private val httpDeserializerGenerator: RestJson1HttpDeserializerGenerator
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
        protocolGeneratorFactory = generator
        model = generator.transformModel(codegenDecorator.transformModel(service, baseModel))
        val baseProvider = RustCodegenPlugin.baseSymbolProvider(model, service, symbolVisitorConfig)
        symbolProvider =
            codegenDecorator.symbolProvider(generator.symbolProvider(model, baseProvider))

        codegenContext = CodegenContext(model, symbolProvider, service, protocol, settings)

        rustCrate = RustCrate(context.fileManifest, symbolProvider, DefaultPublicModules)
        protocolGenerator = protocolGeneratorFactory.buildProtocolGenerator(codegenContext)

        httpBindingResolver =
            HttpTraitHttpBindingResolver(
                codegenContext.model,
                ProtocolContentTypes.consistent("application/json"),
            )

        when (codegenContext.protocol) {
            RestJson1Trait.ID -> {
                httpSerializerGenerator =
                    RestJson1HttpSerializerGenerator(codegenContext, httpBindingResolver)
                httpDeserializerGenerator =
                    RestJson1HttpDeserializerGenerator(codegenContext, httpBindingResolver)
            }
            else -> {
                TODO("Protocol ${codegenContext.protocol} not supported yet")
            }
        }
    }

    /**
     * Base model transformation applied to all services.
     * See below for details.
     */
    private fun baselineTransform(model: Model) =
        model
            // Add errors attached at the service level to the models
            .let { ModelTransformer.create().copyServiceErrorsToOperations(it, settings.getService(it)) }
            // Add `Box<T>` to recursive shapes as necessary
            .let(RecursiveShapeBoxer::transform)
            // Normalize the `message` field on errors when enabled in settings (default: true)
            .letIf(settings.codegenConfig.addMessageToErrors, AddErrorMessage::transform)
            // Normalize operations by adding synthetic input and output shapes to every operation
            .let(OperationNormalizer::transform)
            // Drop unsupported event stream operations from the model
            .let { RemoveEventStreamOperations.transform(it, settings) }
            // Normalize event stream operations
            .let(EventStreamNormalizer::transform)

    /**
     * Execute code generation
     *
     * 1. Load the service from `RustSettings`.
     * 2. Traverse every shape in the closure of the service.
     * 3. Loop through each shape and visit them (calling the override functions in this class)
     * 4. Call finalization tasks specified by decorators.
     * 5. Write the in-memory buffers out to files.
     *
     * The main work of code generation (serializers, protocols, etc.) is handled in `fn serviceShape` below.
     */
    fun execute() {
        val service = settings.getService(model)
        logger.info(
            "[rust-server-codegen] Generating Rust server for service $service, protocol ${codegenContext.protocol}"
        )
        val serviceShapes = Walker(model).walkShapes(service)
        serviceShapes.forEach { it.accept(this) }
        codegenDecorator.extras(codegenContext, rustCrate)
        val module = RustMetadata(public = true)
        rustCrate.withModule(
            RustModule(
                "error",
                module,
                documentation = "All error types that operations can respond with."
            )
        ) { writer -> renderSerdeError(writer) }
        rustCrate.finalize(
            settings,
            model,
            codegenDecorator.crateManifestCustomizations(codegenContext),
            codegenDecorator.libRsCustomizations(codegenContext, listOf()),
            // TODO: Remove `requireDocs` and always require them once the server codegen is far enough along
            requireDocs = false
        )
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

    /**
     * Operation Shape Visitor
     *
     * For each operation shape, generate the corresponding protocol implementation.
     */
    override fun operationShape(shape: OperationShape?) {
        logger.info("[rust-server-codegen] Generating operation $shape")
        if (shape != null) {
            rustCrate.useShapeWriter(shape) { writer ->
                shape.let {
                    httpDeserializerGenerator.render(writer, it)
                    httpSerializerGenerator.render(writer, it)
                }
            }
        }
    }

    /**
     * Structure Shape Visitor
     *
     * For each structure shape, generate:
     * - A Rust structure for the shape (`StructureGenerator`).
     * - A builder for the shape.
     *
     * This function _does not_ generate any serializers.
     */
    override fun structureShape(shape: StructureShape) {
        logger.info("[rust-server-codegen] Generating a structure $shape")
        rustCrate.useShapeWriter(shape) { writer ->
            StructureGenerator(model, symbolProvider, writer, shape).render()
            if (!shape.hasTrait<SyntheticInputTrait>()) {
                val builderGenerator =
                    BuilderGenerator(codegenContext.model, codegenContext.symbolProvider, shape)
                builderGenerator.render(writer)
                writer.implBlock(shape, symbolProvider) {
                    builderGenerator.renderConvenienceMethod(this)
                }
            }
        }
    }

    /**
     * String Shape Visitor
     *
     * Although raw strings require no code generation, enums are actually `EnumTrait` applied to string shapes.
     */
    override fun stringShape(shape: StringShape) {
        logger.info("[rust-server-codegen] Generating an enum $shape")
        shape.getTrait<EnumTrait>()?.also { enum ->
            rustCrate.useShapeWriter(shape) { writer ->
                EnumGenerator(model, symbolProvider, writer, shape, enum).render()
            }
        }
    }

    /**
     * Union Shape Visitor
     *
     * Generate an `enum` for union shapes.
     *
     * This function _does not_ generate any serializers.
     */
    override fun unionShape(shape: UnionShape) {
        logger.info("[rust-server-codegen] Generating an union $shape")
        rustCrate.useShapeWriter(shape) {
            UnionGenerator(model, symbolProvider, it, shape).render()
        }
    }

    /**
     * Generate service-specific code for the model:
     * - Serializers
     * - Deserializers
     * - Fluent client
     * - Trait implementations
     * - Protocol tests
     * - Operation structures
     */
    override fun serviceShape(shape: ServiceShape) {
        logger.info("[rust-server-codegen] Generating a service $shape")
        ServiceGenerator(
            rustCrate,
            protocolGenerator,
            ProtocolSupport(
                requestDeserialization = true,
                requestBodyDeserialization = true,
                responseSerialization = true,
                errorSerialization = true
            ),
            codegenContext,
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
                    DeserializeJson(aws_smithy_json::deserialize::Error),
                    DeserializeHeader(aws_smithy_http::header::ParseError),
                    DeserializeLabel(std::string::String),
                    BuildInput(aws_smithy_http::operation::BuildError),
                    BuildResponse(http::Error),
                    SmithyType(aws_smithy_types::Error),
                }

                impl Error {
                    ##[allow(dead_code)]
                    pub fn generic(msg: &'static str) -> Self {
                        Self::Generic(msg.into())
                    }
                }

                impl From<aws_smithy_json::deserialize::Error> for Error {
                    fn from(err: aws_smithy_json::deserialize::Error) -> Self {
                        Self::DeserializeJson(err)
                    }
                }

                impl From<aws_smithy_http::header::ParseError> for Error {
                    fn from(err: aws_smithy_http::header::ParseError) -> Self {
                        Self::DeserializeHeader(err)
                    }
                }

                impl From<aws_smithy_http::operation::BuildError> for Error {
                    fn from(err: aws_smithy_http::operation::BuildError) -> Self {
                        Self::BuildInput(err)
                    }
                }

                impl From<aws_smithy_types::Error> for Error {
                    fn from(err: aws_smithy_types::Error) -> Self {
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
