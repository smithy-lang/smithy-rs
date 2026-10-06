/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.fuzz

import software.amazon.smithy.build.FileManifest
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.NullableIndex
import software.amazon.smithy.model.knowledge.TopDownIndex
import software.amazon.smithy.model.node.ArrayNode
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.shapes.BooleanShape
import software.amazon.smithy.model.shapes.EnumShape
import software.amazon.smithy.model.shapes.IntEnumShape
import software.amazon.smithy.model.shapes.ListShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.NumberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeType
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.Local
import software.amazon.smithy.rust.codegen.core.rustlang.RustReservedWords
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CoreCodegenConfig
import software.amazon.smithy.rust.codegen.core.smithy.CoreRustSettings
import software.amazon.smithy.rust.codegen.core.smithy.PublicImportSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProviderConfig
import software.amazon.smithy.rust.codegen.core.smithy.SymbolVisitor
import software.amazon.smithy.rust.codegen.core.smithy.contextName
import software.amazon.smithy.rust.codegen.core.smithy.generators.Instantiator
import software.amazon.smithy.rust.codegen.core.smithy.isOptional
import software.amazon.smithy.rust.codegen.core.smithy.transformers.eventStreamErrors
import software.amazon.smithy.rust.codegen.core.util.findStreamingMember
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.isEventStream
import software.amazon.smithy.rust.codegen.core.util.outputShape
import software.amazon.smithy.rust.codegen.core.util.toPascalCase
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.ServerModuleProvider
import software.amazon.smithy.rust.codegen.server.smithy.isDirectlyConstrained
import java.nio.file.Path
import kotlin.io.path.name

private fun rustSettings(
    fuzzSettings: FuzzSettings,
    target: TargetCrate,
) = CoreRustSettings(
    fuzzSettings.service,
    moduleVersion = "0.1.0",
    moduleName = "fuzz-target-${target.name}",
    moduleAuthors = listOf(),
    codegenConfig = CoreCodegenConfig(),
    license = null,
    runtimeConfig = fuzzSettings.runtimeConfig,
    moduleDescription = null,
    moduleRepository = null,
)

data class FuzzTargetContext(
    val target: TargetCrate,
    val fuzzSettings: FuzzSettings,
    val rustCrate: RustCrate,
    val model: Model,
    val symbolProvider: RustSymbolProvider,
    private val manifest: FileManifest,
) {
    fun finalize(): FileManifest {
        val forceWorkspace =
            mapOf(
                "workspace" to listOf("_ignored" to "_ignored").toMap(),
                "lib" to mapOf("crate-type" to listOf("cdylib")),
            )
        val rustSettings = rustSettings(fuzzSettings, target)
        rustCrate.finalize(rustSettings, model, forceWorkspace, listOf(), requireDocs = false)
        return manifest
    }
}

class FuzzTargetGenerator(private val context: FuzzTargetContext) {
    private val model = context.model
    private val serviceShape = context.model.expectShape(context.fuzzSettings.service, ServiceShape::class.java)
    private val symbolProvider = PublicImportSymbolProvider(context.symbolProvider, targetCrate().name)

    private fun targetCrate(): RuntimeType {
        val path = Path.of(context.target.relativePath).toAbsolutePath()
        return CargoDependency(
            name = path.name,
            location = Local(path.parent?.toString() ?: ""),
            `package` = context.target.targetPackage(),
        ).toType()
    }

    private val smithyFuzz = context.fuzzSettings.runtimeConfig.smithyRuntimeCrate("smithy-fuzz").toType()
    private val ctx =
        arrayOf(
            "fuzz_harness" to smithyFuzz.resolve("fuzz_harness"),
            "fuzz_service" to smithyFuzz.resolve("fuzz_service"),
            "FuzzResult" to smithyFuzz.resolve("FuzzResult"),
            "Body" to smithyFuzz.resolve("Body"),
            "http" to CargoDependency.Http1x.toType(),
            "target" to targetCrate(),
        )

    private val serviceName = context.fuzzSettings.service.name.toPascalCase()

    // A schema-serde server's builder is not generic over the request body, so it takes no `Body` type argument.
    private val builderGenerics =
        if (context.target.isSchemaServer()) "" else "::<#{Body}, _, _, _>"

    fun generateFuzzTarget() {
        context.rustCrate.lib {
            rustTemplate(
                """
                #{fuzz_harness}!(|tx| {
                    let config = #{target}::${serviceName}Config::builder().build();
                    #{tx_clones}
                    #{target}::$serviceName::builder$builderGenerics(config)#{all_operations}.build_unchecked()
                });

                """,
                *ctx,
                "all_operations" to allOperations(),
                "tx_clones" to allTxs(),
                *preludeScope,
            )
        }
    }

    private fun operationsToImplement(): List<OperationShape> {
        val index = TopDownIndex.of(model)
        return index.getContainedOperations(serviceShape).filter { operationShape ->
            streamingSupported(operationShape) &&
                // TODO(fuzzing): it should be possible to work backwards from constraints to satisfy them in most cases.
                (
                    !operationShape.outputShape(model).isDirectlyConstrained(symbolProvider) ||
                        requiredOutputMembers(operationShape).all { canDefault(it) }
                )
        }.toList()
    }

    /**
     * Event stream operations are implemented: the handler drains the incoming events into the
     * comparison summary and emits a deterministic example of each output event variant. Plain streaming blobs are not
     * implemented yet. Server codegen flattens stream members to non-optional fields, so the
     * handler reads and sets them directly.
     */
    private fun streamingSupported(operation: OperationShape): Boolean {
        val streamingMembers =
            listOfNotNull(
                operation.inputShape(model).findStreamingMember(model),
                operation.outputShape(model).findStreamingMember(model),
            )
        return streamingMembers.all { member -> member.isEventStream(model) }
    }

    private fun requiredOutputMembers(operation: OperationShape): List<MemberShape> =
        operation.outputShape(model).members().filter { member ->
            member.isRequired && !member.isEventStream(model)
        }

    /** Whether the member's Rust type implements `Default`, so a handler can satisfy `@required` with it. */
    private fun canDefault(member: MemberShape): Boolean {
        val target = model.expectShape(member.target)
        if (target.isDirectlyConstrained(symbolProvider)) {
            return false
        }
        return when (target) {
            is EnumShape, is IntEnumShape -> false
            is StringShape -> !target.hasTrait(EnumTrait::class.java)
            is NumberShape, is BooleanShape, is ListShape, is MapShape -> true
            else -> false
        }
    }

    /** Event shapes cannot carry constraints. Bound recursion and use empty collections at the limit. */
    private fun eventValue(
        shape: Shape,
        depth: Int = 0,
    ): Node {
        check(depth < 16) { "cannot construct a finite event value for ${shape.id}" }
        return when (shape.type) {
            ShapeType.STRUCTURE -> {
                val builder = Node.objectNodeBuilder()
                shape.members().filter { depth < 3 || it.isRequired }.forEach {
                    builder.withMember(it.memberName, eventValue(model.expectShape(it.target), depth + 1))
                }
                builder.build()
            }
            ShapeType.UNION -> {
                val member = shape.members().first()
                Node.objectNode().withMember(member.memberName, eventValue(model.expectShape(member.target), depth + 1))
            }
            ShapeType.LIST, ShapeType.SET ->
                if (depth >= 3) {
                    Node.arrayNode()
                } else {
                    ArrayNode.fromNodes(eventValue(model.expectShape(shape.allMembers.getValue("member").target), depth + 1))
                }
            ShapeType.MAP -> Node.objectNode()
            ShapeType.STRING, ShapeType.BLOB -> Node.from("fuzz")
            ShapeType.ENUM -> Node.from((shape as EnumShape).enumValues.values.first())
            ShapeType.INT_ENUM -> Node.from((shape as IntEnumShape).enumValues.values.first())
            ShapeType.BOOLEAN -> Node.from(true)
            ShapeType.TIMESTAMP -> Node.from(1)
            ShapeType.DOCUMENT -> Node.objectNode().withMember("value", "fuzz")
            else -> Node.from(1)
        }
    }

    private fun RustWriter.outputEvents(member: MemberShape) {
        val union = model.expectShape(member.target, UnionShape::class.java)
        val instantiator =
            Instantiator(
                symbolProvider, model, context.fuzzSettings.runtimeConfig,
                object : Instantiator.BuilderKindBehavior {
                    override fun hasFallibleBuilder(shape: StructureShape) = shape.isDirectlyConstrained(symbolProvider)

                    override fun setterName(memberShape: MemberShape) = symbolProvider.toMemberName(memberShape)

                    override fun doesSetterTakeInOption(memberShape: MemberShape) =
                        symbolProvider.toSymbol(memberShape).isOptional()
                },
            )
        rustTemplate(
            "#{target}::types::EventStreamSender::from(#{futures}::stream::iter(::std::vec![#{events}]))",
            *ctx, *preludeScope,
            "futures" to ServerCargoDependency.FuturesUtil.toType(),
            "events" to
                writable {
                    union.members().forEach { variant ->
                        val event = model.expectShape(variant.target)
                        rustTemplate(
                            "#{Ok}(#{Union}::${symbolProvider.toMemberName(variant)}(#{value})),",
                            *preludeScope,
                            "Union" to symbolProvider.toSymbol(union),
                            "value" to instantiator.generate(event, eventValue(event)),
                        )
                    }
                    union.eventStreamErrors().forEach { errorMember ->
                        val error = model.expectShape((errorMember as MemberShape).target)
                        rustTemplate(
                            "#{Err}(#{target}::error::${symbolProvider.toSymbol(union).name}Error::${symbolProvider.toSymbol(error).name}(#{value})),",
                            *ctx, *preludeScope,
                            "value" to instantiator.generate(error, eventValue(error)),
                        )
                    }
                },
        )
    }

    private fun allTxs(): Writable =
        writable {
            operationsToImplement().forEach { op ->
                val operationName =
                    op.contextName(serviceShape).toSnakeCase().let { RustReservedWords.escapeIfNeeded(it) }
                rust("let tx_$operationName = tx.clone();")
            }
        }

    private fun allOperations(): Writable =
        writable {
            val operations = operationsToImplement()
            operations.forEach { op ->
                val operationName =
                    op.contextName(serviceShape).toSnakeCase().let { RustReservedWords.escapeIfNeeded(it) }
                val outputStreamMember = op.outputShape(model).findStreamingMember(model)
                val output =
                    writable {
                        val outputShape = op.outputShape(model)
                        val setters =
                            requiredOutputMembers(op).joinToString("") {
                                ".${symbolProvider.toMemberName(it)}(Default::default())"
                            } +
                                (
                                    outputStreamMember?.let {
                                        ".${symbolProvider.toMemberName(it)}(#{output_stream})"
                                    } ?: ""
                                )
                        // Server codegen treats an event stream member as required, so its builder is fallible.
                        val fallible = outputShape.isDirectlyConstrained(symbolProvider) || outputStreamMember != null
                        val unwrap = if (fallible) ".unwrap()" else ""
                        val body =
                            if (op.errors.isEmpty()) {
                                "#{Output}::builder()$setters.build()$unwrap"
                            } else {
                                "Ok(#{Output}::builder()$setters.build()$unwrap)"
                            }
                        rustTemplate(
                            body,
                            "Output" to symbolProvider.toSymbol(op.outputShape(model)),
                            "output_stream" to
                                writable {
                                    outputStreamMember?.let { outputEvents(it) }
                                },
                            *ctx,
                        )
                    }
                val inputStreamMember = op.inputShape(model).findStreamingMember(model)
                val summarizeInput =
                    writable {
                        if (inputStreamMember == null) {
                            rust("""tx.send(format!("{:?}", input)).await.unwrap();""")
                            return@writable
                        }
                        // The stream receiver's `Debug` output names implementation internals, so the
                        // summary is built from the non-stream members and the decoded events. Decode
                        // failures are normalized: the two targets phrase their errors differently, and
                        // only *what* each target accepts or rejects should be compared.
                        val nonStreamDebugs =
                            op.inputShape(model).members()
                                .filter { it.memberName != inputStreamMember.memberName }
                                .joinToString("") {
                                    """summary.push(format!("{:?}", input.${symbolProvider.toMemberName(it)}));"""
                                }
                        rust(
                            """
                            let mut summary = Vec::<String>::new();
                            summary.push("$operationName".to_owned());
                            $nonStreamDebugs
                            let mut events = input.${symbolProvider.toMemberName(inputStreamMember)};
                            loop {
                                match events.recv().await {
                                    Ok(Some(event)) => summary.push(format!("{:?}", event)),
                                    Ok(None) => break,
                                    Err(error) => {
                                        summary.push(match error.as_service_error() {
                                            Some(error) => format!("<event-stream-service-error:{:?}>", error),
                                            None => "<event-stream-error>".to_owned(),
                                        });
                                        break;
                                    }
                                }
                            }
                            tx.send(format!("{:?}", summary)).await.unwrap();
                            """,
                        )
                    }
                rustTemplate(
                    """
                    .$operationName(move |input: #{Input}| {
                        let tx = tx_$operationName.clone();
                        async move {
                            #{summarize_input:W}
                            #{output}
                        }
                })""",
                    "Input" to symbolProvider.toSymbol(op.inputShape(model)),
                    "output" to output,
                    "summarize_input" to summarizeInput,
                    *preludeScope,
                )
            }
        }
}

fun createFuzzTarget(
    target: TargetCrate,
    baseManifest: FileManifest,
    fuzzSettings: FuzzSettings,
    model: Model,
): FuzzTargetContext {
    val newManifest = FileManifest.create(baseManifest.resolvePath(Path.of(target.name)))
    val codegenConfig = CoreCodegenConfig()
    val symbolProvider =
        SymbolVisitor(
            rustSettings(fuzzSettings, target),
            model,
            model.expectShape(fuzzSettings.service, ServiceShape::class.java),
            RustSymbolProviderConfig(
                fuzzSettings.runtimeConfig,
                renameExceptions = false,
                NullableIndex.CheckMode.SERVER,
                ServerModuleProvider,
            ),
        )
    val crate =
        RustCrate(
            newManifest,
            symbolProvider,
            codegenConfig,
            NoOpDocProvider(),
        )
    return FuzzTargetContext(
        target = target,
        fuzzSettings = fuzzSettings,
        rustCrate = crate,
        model = model,
        manifest = newManifest,
        symbolProvider = symbolProvider,
    )
}
