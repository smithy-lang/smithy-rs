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
import software.amazon.smithy.model.traits.HttpPayloadTrait
import software.amazon.smithy.model.traits.StreamingTrait
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
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProviderConfig
import software.amazon.smithy.rust.codegen.core.smithy.SymbolVisitor
import software.amazon.smithy.rust.codegen.core.smithy.contextName
import software.amazon.smithy.rust.codegen.core.smithy.generators.Instantiator
import software.amazon.smithy.rust.codegen.core.smithy.generators.InstantiatorCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.InstantiatorSection
import software.amazon.smithy.rust.codegen.core.smithy.isOptional
import software.amazon.smithy.rust.codegen.core.smithy.transformers.eventStreamErrors
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.findStreamingMember
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.isEventStream
import software.amazon.smithy.rust.codegen.core.util.outputShape
import software.amazon.smithy.rust.codegen.core.util.toPascalCase
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase
import software.amazon.smithy.rust.codegen.server.smithy.ConstrainedShapeSymbolProvider
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.ServerModuleProvider
import software.amazon.smithy.rust.codegen.server.smithy.isDirectlyConstrained
import software.amazon.smithy.rust.codegen.server.smithy.transformers.ConstrainedMemberTransform
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
    private val symbolProvider = FuzzPublicSymbols(context.symbolProvider, targetCrate().name)

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
    private val inputRecords =
        FuzzInputRecords(
            model,
            symbolProvider,
            RuntimeType.smithyTypes(context.fuzzSettings.runtimeConfig).resolve("Document"),
            RuntimeType.smithyTypes(context.fuzzSettings.runtimeConfig).resolve("Number"),
        )

    private val scenarios =
        TopDownIndex.of(model).getContainedOperations(serviceShape).associateWith { fuzzResponseScenarios(model, it) }

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
            inputRecords.render(this)
        }
    }

    private fun operationsToImplement(): List<OperationShape> {
        val index = TopDownIndex.of(model)
        return index.getContainedOperations(serviceShape).filter { operationShape ->
            streamingSupported(operationShape) &&
                // TODO(fuzzing): it should be possible to work backwards from constraints to satisfy them in most cases.
                (
                    scenarios.getValue(operationShape).any { !it.error } ||
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

    private fun scenarioOutput(operation: OperationShape): Writable? {
        val cases = scenarios.getValue(operation)
        if (cases.isEmpty()) return null
        val selector =
            operation.inputShape(model).allMembers["scenario"]
                ?: error("fuzz scenario operation ${operation.id} needs a String input member named scenario")
        require(model.expectShape(selector.target) is StringShape) { "scenario selector must be a String" }
        val fallback = cases.find { it.name == "populated" && !it.error } ?: cases.first { !it.error }
        val instantiator =
            Instantiator(
                symbolProvider, model, context.fuzzSettings.runtimeConfig,
                object : Instantiator.BuilderKindBehavior {
                    override fun hasFallibleBuilder(shape: StructureShape) =
                        shape.isDirectlyConstrained(symbolProvider) || shape.findStreamingMember(model) != null

                    override fun setterName(memberShape: MemberShape) = symbolProvider.toMemberName(memberShape)

                    override fun doesSetterTakeInOption(memberShape: MemberShape) =
                        symbolProvider.toSymbol(memberShape).isOptional()
                },
                customizations =
                    listOf(
                        object : InstantiatorCustomization() {
                            override fun section(section: InstantiatorSection): Writable =
                                writable {
                                    val shape =
                                        when (section) {
                                            is InstantiatorSection.BeforeInstantiatingValue -> section.shape
                                            is InstantiatorSection.AfterInstantiatingValue -> section.shape
                                        }
                                    val convert =
                                        shape !is EnumShape && shape !is IntEnumShape &&
                                            !shape.hasTrait(EnumTrait::class.java) &&
                                            !shape.hasTrait(StreamingTrait::class.java) &&
                                            shape !is StructureShape &&
                                            shape.isDirectlyConstrained(symbolProvider)
                                    if (convert) {
                                        when (section) {
                                            is InstantiatorSection.BeforeInstantiatingValue -> if (shape is NumberShape) rust("(")
                                            is InstantiatorSection.AfterInstantiatingValue -> {
                                                if (shape is NumberShape) rust(")")
                                                rust(""".try_into().expect("declared fuzz response satisfies model constraints")""")
                                            }
                                        }
                                    }
                                }
                        },
                    ),
            )

        fun result(case: FuzzResponseScenario) =
            writable {
                // The protocol-test Instantiator invents an empty structure for an omitted
                // structure payload. A response scenario must preserve actual absence.
                val params =
                    case.shape.members().filter {
                        it.hasTrait(HttpPayloadTrait::class.java) &&
                            symbolProvider.toSymbol(it).isOptional() &&
                            model.expectShape(it.target) is StructureShape &&
                            !case.test.params.containsMember(it.memberName)
                    }.fold(case.test.params) { params, member -> params.withMember(member.memberName, Node.nullNode()) }
                if (case.error) {
                    rustTemplate(
                        "#{Err}(#{target}::error::${symbolProvider.toSymbol(operation).name}Error::${symbolProvider.toSymbol(case.shape).name}(#{Value}))",
                        "Value" to instantiator.generate(case.shape, params),
                        *ctx, *preludeScope,
                    )
                } else if (operation.errors.isNotEmpty()) {
                    rustTemplate("#{Ok}(#{Value})", "Value" to instantiator.generate(case.shape, params), *preludeScope)
                } else {
                    instantiator.generate(case.shape, params)(this)
                }
            }
        return writable {
            val field = symbolProvider.toMemberName(selector)
            val select =
                if (symbolProvider.toSymbol(selector).isOptional()) {
                    "input.$field.as_ref().map(|value| value.as_str()).unwrap_or(${fallback.name.dq()})"
                } else {
                    "input.$field.as_str()"
                }
            rust("match $select {")
            cases.forEach {
                rustTemplate("${it.name.dq()} => #{Result},", "Result" to result(it))
            }
            rustTemplate("_ => #{Result},", "Result" to result(fallback))
            rust("}")
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
                    scenarioOutput(op)
                        ?: writable {
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
                            rust("""tx.send(${inputRecords.record(op.inputShape(model), "&input")}).await.unwrap();""")
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
                                    """summary.push(${inputRecords.member(it, "&input.${symbolProvider.toMemberName(it)}")});"""
                                }
                        val inputUnion = model.expectShape(inputStreamMember.target, UnionShape::class.java)
                        val serviceErrors = inputUnion.eventStreamErrors().map { model.expectShape((it as MemberShape).target) }
                        val serviceErrorBinding = if (serviceErrors.isEmpty()) "_error" else "error"
                        val serviceErrorRecord =
                            if (serviceErrors.isEmpty()) {
                                "\"<event-stream-service-error>\".to_owned()"
                            } else {
                                val arms =
                                    serviceErrors.joinToString("\n") {
                                        "${targetCrate().name}::error::${symbolProvider.toSymbol(inputUnion).name}Error::${symbolProvider.toSymbol(it).name}(value) => format!(\"<event-stream-service-error:{}>\", ${inputRecords.record(it, "value")}),"
                                    }
                                "match error { $arms }"
                            }
                        rustTemplate(
                            inputRecords.template(
                                """
                            let mut summary = @FuzzVec@::<@FuzzString@>::new();
                            summary.push("$operationName".to_owned());
                            $nonStreamDebugs
                            let mut events = input.${symbolProvider.toMemberName(inputStreamMember)};
                            loop {
                                match events.recv().await {
                                    @FuzzOk@(@FuzzSome@(event)) => summary.push(${inputRecords.record(model.expectShape(inputStreamMember.target), "&event")}),
                                    @FuzzOk@(@FuzzNone@) => break,
                                    @FuzzErr@(error) => {
                                        summary.push(match error.as_service_error() {
                                            @FuzzSome@($serviceErrorBinding) => $serviceErrorRecord,
                                            @FuzzNone@ => "<event-stream-error>".to_owned(),
                                        });
                                        break;
                                    }
                                }
                            }
                            tx.send(format!("{:?}", summary)).await.unwrap();
                            """,
                            ),
                            *preludeScope,
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
    // Match the SDK's public types for constraints attached directly to members.
    val targetModel = ConstrainedMemberTransform.transform(model)
    val symbolProvider =
        ConstrainedShapeSymbolProvider(
            SymbolVisitor(
                rustSettings(fuzzSettings, target),
                targetModel,
                targetModel.expectShape(fuzzSettings.service, ServiceShape::class.java),
                RustSymbolProviderConfig(
                    fuzzSettings.runtimeConfig,
                    renameExceptions = false,
                    NullableIndex.CheckMode.SERVER,
                    ServerModuleProvider,
                ),
            ),
            targetModel.expectShape(fuzzSettings.service, ServiceShape::class.java), publicConstrainedTypes = true,
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
        model = targetModel,
        manifest = newManifest,
        symbolProvider = symbolProvider,
    )
}
