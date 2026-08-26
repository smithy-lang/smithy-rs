/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.serde

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.knowledge.TopDownIndex
import software.amazon.smithy.model.shapes.BigDecimalShape
import software.amazon.smithy.model.shapes.BigIntegerShape
import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.shapes.BooleanShape
import software.amazon.smithy.model.shapes.ByteShape
import software.amazon.smithy.model.shapes.CollectionShape
import software.amazon.smithy.model.shapes.DocumentShape
import software.amazon.smithy.model.shapes.DoubleShape
import software.amazon.smithy.model.shapes.EnumShape
import software.amazon.smithy.model.shapes.FloatShape
import software.amazon.smithy.model.shapes.IntegerShape
import software.amazon.smithy.model.shapes.LongShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.NumberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShortShape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.TimestampShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.model.traits.SparseTrait
import software.amazon.smithy.model.traits.StreamingTrait
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.contextName
import software.amazon.smithy.rust.codegen.core.smithy.generators.BuilderGenerator
import software.amazon.smithy.rust.codegen.core.smithy.isRustBoxed
import software.amazon.smithy.rust.codegen.core.smithy.protocols.parse.ReturnSymbolToParse
import software.amazon.smithy.rust.codegen.core.smithy.protocols.shapeFunctionName
import software.amazon.smithy.rust.codegen.core.smithy.rustType
import software.amazon.smithy.rust.codegen.core.smithy.traits.RustBoxTrait
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.hasEventStreamMember
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.isEventStream
import software.amazon.smithy.rust.codegen.core.util.isTargetUnit
import software.amazon.smithy.rust.codegen.core.util.toPascalCase
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.canReachConstrainedShape
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServerBuilderKindBehavior
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.returnSymbolToParseFn
import software.amazon.smithy.rust.codegen.server.smithy.generators.serverBuilderSymbol
import software.amazon.smithy.rust.codegen.server.smithy.isDirectlyConstrained
import software.amazon.smithy.rust.codegen.server.smithy.traits.ShapeReachableFromOperationInputTagTrait
import software.amazon.smithy.rust.codegen.server.smithy.traits.isReachableFromOperationInput

/**
 * Generates ordinary `Deserialize` implementations for nominal model types.
 *
 * Serde formats are first decoded into a private value tree. Shape-specific parsers then convert that tree into
 * generated model types. This preserves duplicate map entries and lets server input shapes flow through their
 * unconstrained representations before constraints are enforced at the root.
 */
class DeserializeImplGenerator(private val codegenContext: CodegenContext) {
    private val model = codegenContext.model
    private val symbolProvider = codegenContext.symbolProvider
    private val topIndex = TopDownIndex.of(model)
    private val serverContext = codegenContext as? ServerCodegenContext
    private val serverReturnSymbol = serverContext?.let(::returnSymbolToParseFn)

    fun generateRootDeserializerForShape(shape: Shape): Writable =
        when (shape) {
            is ServiceShape ->
                topIndex.getContainedOperations(shape)
                    .map(::generateRootDeserializerForShape)
                    .fold(writable { }) { left, right ->
                        writable {
                            left(this)
                            right(this)
                        }
                    }

            is OperationShape ->
                if (shape.isEventStream(model)) {
                    writable { }
                } else {
                    writable {
                        generateRootDeserializerForShape(model.expectShape(shape.inputShape))(this)
                        generateRootDeserializerForShape(model.expectShape(shape.outputShape))(this)
                        shape.errors.forEach {
                            generateRootDeserializerForShape(model.expectShape(it))(this)
                        }
                    }
                }

            is UnionShape ->
                if (shape.isEventStream()) {
                    writable { }
                } else {
                    writable { addDependency(parser(shape).toSymbol()) }
                }

            is StructureShape ->
                if (shape.hasEventStreamMember(model)) {
                    writable { }
                } else {
                    writable { addDependency(parser(shape).toSymbol()) }
                }

            is EnumShape -> writable { addDependency(parser(shape).toSymbol()) }
            is StringShape ->
                if (shape.hasTrait<EnumTrait>()) {
                    writable { addDependency(parser(shape).toSymbol()) }
                } else {
                    writable { }
                }

            else -> writable { }
        }

    private fun parser(shape: Shape): RuntimeType =
        RuntimeType.forInlineFun(parseFunctionName(shape), DeserializerModule) {
            renderParser(shape)(this)
        }

    private fun parseFunctionName(shape: Shape): String =
        symbolProvider.shapeFunctionName(codegenContext.serviceShape, shape) + "_serde_deserialize"

    private fun parseInfo(shape: Shape): ReturnSymbolToParse {
        if (shape is BlobShape && shape.hasTrait<StreamingTrait>()) {
            return ReturnSymbolToParse(RuntimeType.byteStream(codegenContext.runtimeConfig).toSymbol(), false)
        }
        if (
            serverReturnSymbol != null &&
            shape.isDirectlyConstrained(symbolProvider) &&
            (shape is StringShape || shape is NumberShape || shape is BlobShape)
        ) {
            return serverReturnSymbol.invoke(shape)
        }
        return if (serverReturnSymbol != null && shape.hasTrait<ShapeReachableFromOperationInputTagTrait>()) {
            serverReturnSymbol.invoke(shape)
        } else {
            ReturnSymbolToParse(symbolProvider.toSymbol(shape), false)
        }
    }

    private fun renderParser(shape: Shape): Writable =
        writable {
            val info = parseInfo(shape)
            rustTemplate(
                """
                ##[allow(unused_mut, unused_variables)]
                ##[allow(clippy::match_single_binding, clippy::single_match)]
                fn ${parseFunctionName(shape)}(
                    value: #{Value},
                    settings: &#{DeserializationSettings},
                ) -> #{Result}<#{Return}, #{String}> {
                    let _ = settings;
                    #{Body}
                }
                """,
                "Value" to dynamicValue(),
                "Return" to info.symbol,
                "Body" to parserBody(shape, info),
                *SupportStructures.codegenScope,
                *RuntimeType.preludeScope,
            )
            if (isNominal(shape)) {
                renderNominalDeserializeImpl(shape, info)
            }
        }

    private fun isNominal(shape: Shape): Boolean =
        shape is StructureShape ||
            shape is UnionShape ||
            shape is EnumShape ||
            (shape is StringShape && shape.hasTrait<EnumTrait>())

    private fun parserBody(
        shape: Shape,
        info: ReturnSymbolToParse,
    ): Writable =
        when (shape) {
            is StructureShape -> parseStructure(shape, info)
            is UnionShape -> parseUnion(shape, info)
            is EnumShape -> parseEnum(shape, info)
            is StringShape ->
                if (shape.hasTrait<EnumTrait>()) {
                    parseEnum(shape, info)
                } else {
                    parseString()
                }

            is BooleanShape -> parseBoolean()
            is NumberShape -> parseNumber(shape)
            is BlobShape -> parseBlob(shape)
            is TimestampShape -> parseTimestamp()
            is DocumentShape -> parseDocument(shape)
            is CollectionShape -> parseCollection(shape, info)
            is MapShape -> parseMap(shape, info)
            else -> writable { rust("Err(\"unsupported shape for deserialization\".to_string())") }
        }

    private fun parseBoolean(): Writable =
        writable {
            rustTemplate(
                """
                match value {
                    #{Value}::Bool(value) => #{Ok}(value),
                    _ => #{Err}("expected a boolean".to_string()),
                }
                """,
                "Value" to dynamicValue(),
                *RuntimeType.preludeScope,
            )
        }

    private fun parseString(): Writable =
        writable {
            rustTemplate(
                """
                match value {
                    #{Value}::String(value) => #{Ok}(value),
                    _ => #{Err}("expected a string".to_string()),
                }
                """,
                "Value" to dynamicValue(),
                *RuntimeType.preludeScope,
            )
        }

    private fun parseEnum(
        shape: Shape,
        info: ReturnSymbolToParse,
    ): Writable =
        writable {
            rustTemplate(
                """
                let text = match value {
                    #{Value}::String(value) => value,
                    _ => return #{Err}("expected an enum string".to_string()),
                };
                """,
                "Value" to dynamicValue(),
                *RuntimeType.preludeScope,
            )
            when {
                codegenContext.target == CodegenTarget.SERVER && info.isUnconstrained ->
                    rustTemplate("#{Ok}(text)", *RuntimeType.preludeScope)

                codegenContext.target == CodegenTarget.SERVER ->
                    rustTemplate(
                        "#{Return}::try_from(text).map_err(|err| err.to_string())",
                        "Return" to info.symbol,
                    )

                else ->
                    rustTemplate(
                        "#{Ok}(#{Return}::from(text.as_str()))",
                        "Return" to info.symbol,
                        *RuntimeType.preludeScope,
                    )
            }
        }

    private fun parseNumber(shape: NumberShape): Writable =
        when (shape) {
            is FloatShape -> parseFloat("f32")
            is DoubleShape -> parseFloat("f64")
            is BigIntegerShape -> parseBigNumber(RuntimeType.bigInteger(codegenContext.runtimeConfig))
            is BigDecimalShape -> parseBigNumber(RuntimeType.bigDecimal(codegenContext.runtimeConfig))
            is ByteShape -> parseInteger("i8")
            is ShortShape -> parseInteger("i16")
            is IntegerShape -> parseInteger("i32")
            is LongShape -> parseInteger("i64")
            else -> writable { rust("Err(\"unsupported number shape\".to_string())") }
        }

    private fun parseInteger(rustType: String): Writable =
        writable {
            rustTemplate(
                """
                match value {
                    #{Value}::I64(value) => <$rustType as #{TryFrom}<i64>>::try_from(value)
                        .map_err(|_| "integer is out of range".to_string()),
                    #{Value}::U64(value) => <$rustType as #{TryFrom}<u64>>::try_from(value)
                        .map_err(|_| "integer is out of range".to_string()),
                    _ => #{Err}("expected an integer".to_string()),
                }
                """,
                "Value" to dynamicValue(),
                *RuntimeType.preludeScope,
            )
        }

    private fun parseFloat(rustType: String): Writable =
        writable {
            val parsedF64 = if (rustType == "f64") "value" else "value as $rustType"
            rustTemplate(
                """
                match value {
                    #{Value}::F64(value) => #{Ok}($parsedF64),
                    #{Value}::I64(value) => #{Ok}(value as $rustType),
                    #{Value}::U64(value) => #{Ok}(value as $rustType),
                    #{Value}::String(value) if settings.allow_non_finite_float_strings => match value.as_str() {
                        "NaN" => #{Ok}($rustType::NAN),
                        "Infinity" => #{Ok}($rustType::INFINITY),
                        "-Infinity" => #{Ok}($rustType::NEG_INFINITY),
                        _ => #{Err}("expected a floating-point number".to_string()),
                    },
                    _ => #{Err}("expected a floating-point number".to_string()),
                }
                """,
                "Value" to dynamicValue(),
                *RuntimeType.preludeScope,
            )
        }

    private fun parseBigNumber(type: RuntimeType): Writable =
        writable {
            rustTemplate(
                """
                let text = match value {
                    #{Value}::String(value) => value,
                    #{Value}::I64(value) => value.to_string(),
                    #{Value}::U64(value) => value.to_string(),
                    #{Value}::F64(value) => value.to_string(),
                    _ => return #{Err}("expected a number".to_string()),
                };
                <#{Type} as ::std::str::FromStr>::from_str(&text).map_err(|err| err.to_string())
                """,
                "Value" to dynamicValue(),
                "Type" to type,
                *RuntimeType.preludeScope,
            )
        }

    private fun parseBlob(shape: BlobShape): Writable =
        writable {
            rustTemplate(
                """
                let bytes = match value {
                    #{Value}::Bytes(value) => value,
                    #{Value}::String(value) => {
                        if value == "streaming data" {
                            return #{Err}("the streaming data placeholder cannot be deserialized".to_string());
                        }
                        #{base64_decode}(value).map_err(|_| "invalid base64 blob".to_string())?
                    },
                    _ => return #{Err}("expected a base64 string or bytes".to_string()),
                };
                """,
                "Value" to dynamicValue(),
                "base64_decode" to RuntimeType.base64Decode(codegenContext.runtimeConfig),
                *RuntimeType.preludeScope,
            )
            if (shape.hasTrait<StreamingTrait>()) {
                rustTemplate(
                    "#{Ok}(#{ByteStream}::from(bytes))",
                    "ByteStream" to RuntimeType.byteStream(codegenContext.runtimeConfig),
                    *RuntimeType.preludeScope,
                )
            } else {
                rustTemplate(
                    "#{Ok}(#{Blob}::new(bytes))",
                    "Blob" to RuntimeType.blob(codegenContext.runtimeConfig),
                    *RuntimeType.preludeScope,
                )
            }
        }

    private fun parseTimestamp(): Writable =
        writable {
            rustTemplate(
                """
                let text = match value {
                    #{Value}::String(value) => value,
                    _ => return #{Err}("expected a timestamp string".to_string()),
                };
                #{DateTime}::from_str(&text, #{Format}::DateTime).map_err(|err| err.to_string())
                """,
                "Value" to dynamicValue(),
                "DateTime" to RuntimeType.dateTime(codegenContext.runtimeConfig),
                "Format" to RuntimeType.smithyTypes(codegenContext.runtimeConfig).resolve("date_time::Format"),
                *RuntimeType.preludeScope,
            )
        }

    private fun parseDocument(shape: DocumentShape): Writable =
        writable {
            val parseName = parseFunctionName(shape)
            rustTemplate(
                """
                let document = match value {
                    #{Value}::Null => #{Document}::Null,
                    #{Value}::Bool(value) => #{Document}::Bool(value),
                    #{Value}::String(value) => #{Document}::String(value),
                    #{Value}::I64(value) if value < 0 => #{Document}::Number(#{Number}::NegInt(value)),
                    #{Value}::I64(value) => #{Document}::Number(#{Number}::PosInt(value as u64)),
                    #{Value}::U64(value) => #{Document}::Number(#{Number}::PosInt(value)),
                    #{Value}::F64(value) => #{Document}::Number(#{Number}::Float(value)),
                    #{Value}::Seq(values) => {
                        let mut result = #{Vec}::with_capacity(values.len());
                        for value in values {
                            result.push($parseName(value, settings)?);
                        }
                        #{Document}::Array(result)
                    },
                    #{Value}::Map(entries) => {
                        let mut result = #{HashMap}::with_capacity(entries.len());
                        for (key, value) in entries {
                            let key = match key {
                                #{Value}::String(key) => key,
                                _ => return #{Err}("document object keys must be strings".to_string()),
                            };
                            result.insert(key, $parseName(value, settings)?);
                        }
                        #{Document}::Object(result)
                    },
                    #{Value}::Bytes(_) => return #{Err}("documents cannot contain raw bytes".to_string()),
                };
                #{Ok}(document)
                """,
                "Value" to dynamicValue(),
                "Document" to RuntimeType.document(codegenContext.runtimeConfig),
                "Number" to RuntimeType.smithyTypes(codegenContext.runtimeConfig).resolve("Number"),
                "HashMap" to RuntimeType.HashMap,
                *RuntimeType.preludeScope,
            )
        }

    private fun parseCollection(
        shape: CollectionShape,
        info: ReturnSymbolToParse,
    ): Writable =
        writable {
            val memberTarget = model.expectShape(shape.member.target)
            val memberParser = parser(memberTarget)
            rustTemplate(
                """
                let values = match value {
                    #{Value}::Seq(values) => values,
                    _ => return #{Err}("expected a sequence".to_string()),
                };
                let mut result = #{Vec}::with_capacity(values.len());
                for value in values {
                    #{ParseMember}
                }
                #{Finish}
                """,
                "Value" to dynamicValue(),
                "ParseMember" to
                    writable {
                        if (shape.hasTrait<SparseTrait>()) {
                            rustTemplate(
                                """
                                if matches!(value, #{Value}::Null) {
                                    result.push(#{None});
                                } else {
                                    result.push(#{Some}(#{MemberParser}(value, settings)?));
                                }
                                """,
                                "Value" to dynamicValue(),
                                "MemberParser" to memberParser,
                                *RuntimeType.preludeScope,
                            )
                        } else {
                            rustTemplate("result.push(#{MemberParser}(value, settings)?);", "MemberParser" to memberParser)
                        }
                    },
                "Finish" to finishContainer(shape, info),
                *RuntimeType.preludeScope,
            )
        }

    private fun parseMap(
        shape: MapShape,
        info: ReturnSymbolToParse,
    ): Writable =
        writable {
            val keyParser = parser(model.expectShape(shape.key.target))
            val valueParser = parser(model.expectShape(shape.value.target))
            rustTemplate(
                """
                let entries = match value {
                    #{Value}::Map(entries) => entries,
                    _ => return #{Err}("expected a map".to_string()),
                };
                let mut result = #{HashMap}::with_capacity(entries.len());
                for (key, value) in entries {
                    let key = #{KeyParser}(key, settings)?;
                    #{ParseValue}
                    if result.insert(key, value).is_some() {
                        return #{Err}("duplicate map key".to_string());
                    }
                }
                #{Finish}
                """,
                "Value" to dynamicValue(),
                "HashMap" to RuntimeType.HashMap,
                "KeyParser" to keyParser,
                "ParseValue" to
                    writable {
                        if (shape.hasTrait<SparseTrait>()) {
                            rustTemplate(
                                """
                                let value = if matches!(value, #{Value}::Null) {
                                    #{None}
                                } else {
                                    #{Some}(#{ValueParser}(value, settings)?)
                                };
                                """,
                                "Value" to dynamicValue(),
                                "ValueParser" to valueParser,
                                *RuntimeType.preludeScope,
                            )
                        } else {
                            rustTemplate(
                                "let value = #{ValueParser}(value, settings)?;",
                                "ValueParser" to valueParser,
                            )
                        }
                    },
                "Finish" to finishContainer(shape, info),
                *RuntimeType.preludeScope,
            )
        }

    private fun finishContainer(
        shape: Shape,
        info: ReturnSymbolToParse,
    ): Writable =
        writable {
            if (info.isUnconstrained) {
                rustTemplate("#{Ok}(#{Return}(result))", "Return" to info.symbol, *RuntimeType.preludeScope)
            } else if (serverContext != null && shape.canReachConstrainedShape(model, symbolProvider)) {
                rustTemplate(
                    """
                    <#{Return} as #{TryFrom}<_>>::try_from(result).map_err(|err| err.to_string())
                    """,
                    "Return" to info.symbol,
                    *RuntimeType.preludeScope,
                )
            } else {
                rustTemplate("#{Ok}(result)", *RuntimeType.preludeScope)
            }
        }

    private fun parseStructure(
        shape: StructureShape,
        info: ReturnSymbolToParse,
    ): Writable =
        writable {
            val builder = structureBuilderSymbol(shape)
            rustTemplate(
                """
                let entries = match value {
                    #{Value}::Map(entries) => entries,
                    _ => return #{Err}("expected a structure map".to_string()),
                };
                let mut builder = #{Builder}::default();
                let mut seen: #{HashSet}<&'static str> = #{HashSet}::new();
                for (key, value) in entries {
                    let key = match key {
                        #{Value}::String(key) => key,
                        _ => return #{Err}("structure field names must be strings".to_string()),
                    };
                    match key.as_str() {
                        #{Members}
                        _ => {}
                    }
                }
                #{Finish}
                """,
                "Value" to dynamicValue(),
                "Builder" to builder,
                "HashSet" to RuntimeType.std.resolve("collections::HashSet"),
                "Members" to
                    writable {
                        shape.members().forEach { member ->
                            renderStructureMember(shape, member)
                        }
                    },
                "Finish" to finishStructure(shape, info),
                *RuntimeType.preludeScope,
            )
        }

    private fun structureBuilderSymbol(shape: StructureShape): Symbol =
        when {
            serverContext == null -> symbolProvider.symbolForBuilder(shape)
            shape.isReachableFromOperationInput() -> shape.serverBuilderSymbol(serverContext)
            else -> shape.serverBuilderSymbol(symbolProvider, false)
        }

    private fun RustWriter.renderStructureMember(
        container: StructureShape,
        member: MemberShape,
    ) {
        val target = model.expectShape(member.target)
        val targetInfo = parseInfo(target)
        val fieldName = symbolProvider.toMemberName(member)
        val wireName = member.memberName
        rustTemplate(
            """
            ${wireName.dq()} => {
                if !seen.insert(${wireName.dq()}) {
                    return #{Err}("duplicate field `$wireName`".to_string());
                }
                let parsed = #{TargetParser}(value, settings)
                    .map_err(|err| format!("invalid field `$wireName`: {err}"))?;
                #{Prepare}
                builder.$fieldName = #{Some}(parsed);
            },
            """,
            "TargetParser" to parser(target),
            "Prepare" to
                prepareMember(
                    container,
                    target,
                    targetInfo,
                    symbolProvider.toSymbol(member).isRustBoxed(),
                ),
            *RuntimeType.preludeScope,
        )
    }

    private fun prepareMember(
        container: Shape,
        target: Shape,
        targetInfo: ReturnSymbolToParse,
        boxed: Boolean,
    ): Writable =
        writable {
            val serverInput =
                serverContext != null &&
                    when (container) {
                        is StructureShape -> container.isReachableFromOperationInput()
                        is UnionShape -> container.isReachableFromOperationInput()
                        else -> false
                    }
            val hiddenDirectConstraint =
                serverContext != null &&
                    !serverContext.settings.codegenConfig.publicConstrainedTypes &&
                    target.isDirectlyConstrained(symbolProvider)
            if (serverContext != null && !serverInput && (requiresConversion(targetInfo, target) || hiddenDirectConstraint)) {
                convertServerOutputMember(target, targetInfo)(this)
            }
            when {
                serverInput && boxed && targetInfo.isUnconstrained ->
                    rustTemplate(
                        "let parsed = #{Box}::new(parsed.into());",
                        *RuntimeType.preludeScope,
                    )

                boxed ->
                    rustTemplate(
                        "let parsed = #{Box}::new(parsed);",
                        *RuntimeType.preludeScope,
                    )

                serverInput && targetInfo.isUnconstrained ->
                    rust("let parsed = parsed.into();")
            }
        }

    private fun convertServerOutputMember(
        target: Shape,
        targetInfo: ReturnSymbolToParse,
    ): Writable =
        writable {
            val server = checkNotNull(serverContext)
            val finalSymbol = symbolProvider.toSymbol(target)
            val usesIntermediateConstrainedType =
                target !is StructureShape &&
                    target !is UnionShape &&
                    target !is EnumShape &&
                    !(target is StringShape && target.hasTrait<EnumTrait>()) &&
                    (
                        !server.settings.codegenConfig.publicConstrainedTypes ||
                            !target.isDirectlyConstrained(symbolProvider)
                    )
            if (usesIntermediateConstrainedType) {
                val constrainedSymbol =
                    if (target.isDirectlyConstrained(symbolProvider)) {
                        server.constrainedShapeSymbolProvider.toSymbol(target)
                    } else {
                        server.pubCrateConstrainedShapeSymbolProvider.toSymbol(target)
                    }
                rustTemplate(
                    """
                    let parsed = <#{Constrained} as #{TryFrom}<#{Parsed}>>::try_from(parsed)
                        .map_err(|err| err.to_string())?;
                    let parsed: #{Final} = parsed.into();
                    """,
                    "Constrained" to constrainedSymbol,
                    "Parsed" to targetInfo.symbol,
                    "Final" to finalSymbol,
                    *RuntimeType.preludeScope,
                )
            } else {
                rustTemplate(
                    """
                    let parsed = <#{Final} as #{TryFrom}<#{Parsed}>>::try_from(parsed)
                        .map_err(|err| err.to_string())?;
                    """,
                    "Final" to finalSymbol,
                    "Parsed" to targetInfo.symbol,
                    *RuntimeType.preludeScope,
                )
            }
        }

    private fun finishStructure(
        shape: StructureShape,
        info: ReturnSymbolToParse,
    ): Writable =
        writable {
            if (info.isUnconstrained) {
                rustTemplate("#{Ok}(builder)", *RuntimeType.preludeScope)
                return@writable
            }
            val fallible =
                if (serverContext == null) {
                    BuilderGenerator.hasFallibleBuilder(shape, symbolProvider)
                } else {
                    ServerBuilderKindBehavior(codegenContext).hasFallibleBuilder(shape)
                }
            if (fallible) {
                rust("builder.build().map_err(|err| err.to_string())")
            } else {
                rustTemplate("#{Ok}(builder.build())", *RuntimeType.preludeScope)
            }
        }

    private fun parseUnion(
        shape: UnionShape,
        info: ReturnSymbolToParse,
    ): Writable =
        writable {
            rustTemplate(
                """
                match value {
                    #{Value}::String(name) => match name.as_str() {
                        #{UnitVariants}
                        _ => #{Err}("unknown union variant".to_string()),
                    },
                    #{Value}::Map(mut entries) if entries.len() == 1 => {
                        let (key, value) = entries.pop().expect("length checked");
                        let key = match key {
                            #{Value}::String(key) => key,
                            _ => return #{Err}("union variant names must be strings".to_string()),
                        };
                        match key.as_str() {
                            #{Variants}
                            _ => #{Err}("unknown union variant".to_string()),
                        }
                    },
                    _ => #{Err}("expected an externally tagged union".to_string()),
                }
                """,
                "Value" to dynamicValue(),
                "UnitVariants" to
                    writable {
                        shape.members().filter(MemberShape::isTargetUnit).forEach { member ->
                            rustTemplate(
                                "${member.memberName.dq()} => #{Ok}(#{Return}::${symbolProvider.toMemberName(member)}),",
                                "Return" to info.symbol,
                                *RuntimeType.preludeScope,
                            )
                        }
                    },
                "Variants" to
                    writable {
                        shape.members().forEach { member ->
                            renderUnionVariant(member, info)
                        }
                    },
                *RuntimeType.preludeScope,
            )
        }

    private fun RustWriter.renderUnionVariant(
        member: MemberShape,
        unionInfo: ReturnSymbolToParse,
    ) {
        val variant = symbolProvider.toMemberName(member)
        if (member.isTargetUnit()) {
            rustTemplate(
                """
                ${member.memberName.dq()} => match value {
                    #{Value}::Null => #{Ok}(#{Return}::$variant),
                    _ => #{Err}("unit union variants must contain null".to_string()),
                },
                """,
                "Value" to dynamicValue(),
                "Return" to unionInfo.symbol,
                *RuntimeType.preludeScope,
            )
        } else {
            val target = model.expectShape(member.target)
            val targetInfo = parseInfo(target)
            rustTemplate(
                """
                ${member.memberName.dq()} => {
                    let parsed = #{TargetParser}(value, settings)
                        .map_err(|err| format!("invalid union variant `${member.memberName}`: {err}"))?;
                    #{Prepare}
                    #{Ok}(#{Return}::$variant(parsed))
                },
                """,
                "TargetParser" to parser(target),
                "Prepare" to
                    prepareMember(
                        model.expectShape(member.container),
                        target,
                        targetInfo,
                        member.hasTrait<RustBoxTrait>(),
                    ),
                "Return" to unionInfo.symbol,
                *RuntimeType.preludeScope,
            )
        }
    }

    private fun RustWriter.renderNominalDeserializeImpl(
        shape: Shape,
        info: ReturnSymbolToParse,
    ) {
        val finalSymbol = symbolProvider.toSymbol(shape)
        val seedName = shape.contextName(codegenContext.serviceShape).toPascalCase() + "Seed"
        rustTemplate(
            """
            struct $seedName<'a> {
                settings: &'a #{DeserializationSettings},
            }

            impl<'de> #{serde}::de::DeserializeSeed<'de> for $seedName<'_> {
                type Value = #{Final};

                fn deserialize<D>(self, deserializer: D) -> #{Result}<Self::Value, D::Error>
                where
                    D: #{serde}::Deserializer<'de>,
                {
                    let value = <#{Value} as #{serde}::Deserialize>::deserialize(deserializer)?;
                    let parsed = ${parseFunctionName(shape)}(value, self.settings)
                        .map_err(<D::Error as #{serde}::de::Error>::custom)?;
                    #{Finalize}
                }
            }

            impl<'de> #{serde}::Deserialize<'de> for #{Final} {
                fn deserialize<D>(deserializer: D) -> #{Result}<Self, D::Error>
                where
                    D: #{serde}::Deserializer<'de>,
                {
                    let settings = #{DeserializationSettings}::current();
                    #{serde}::de::DeserializeSeed::deserialize(
                        $seedName { settings: &settings },
                        deserializer,
                    )
                }
            }
            """,
            "Value" to dynamicValue(),
            "Final" to finalSymbol,
            "Finalize" to
                writable {
                    if (requiresConversion(info, shape)) {
                        rustTemplate(
                            """
                            let parsed = <#{Final} as #{TryFrom}<#{Parsed}>>::try_from(parsed)
                                .map_err(<D::Error as #{serde}::de::Error>::custom)?;
                            #{Ok}(parsed)
                            """,
                            "Final" to finalSymbol,
                            "Parsed" to info.symbol,
                            *SupportStructures.codegenScope,
                            *RuntimeType.preludeScope,
                        )
                    } else {
                        rustTemplate("#{Ok}(parsed)", *RuntimeType.preludeScope)
                    }
                },
            *SupportStructures.codegenScope,
            *RuntimeType.preludeScope,
        )
    }

    private fun requiresConversion(
        info: ReturnSymbolToParse,
        finalShape: Shape,
    ): Boolean =
        info.isUnconstrained &&
            info.symbol.rustType() != symbolProvider.toSymbol(finalShape).rustType()

    private fun dynamicValue(): RuntimeType =
        RuntimeType.forInlineFun("SerdeDeserializeValue", DeserializerModule) {
            rustTemplate(
                """
                ##[allow(dead_code)]
                enum SerdeDeserializeValue {
                    Null,
                    Bool(bool),
                    I64(i64),
                    U64(u64),
                    F64(f64),
                    String(#{String}),
                    Bytes(#{Vec}<u8>),
                    Seq(#{Vec}<SerdeDeserializeValue>),
                    Map(#{Vec}<(SerdeDeserializeValue, SerdeDeserializeValue)>),
                }

                struct SerdeDeserializeValueVisitor;

                impl<'de> #{serde}::de::Visitor<'de> for SerdeDeserializeValueVisitor {
                    type Value = SerdeDeserializeValue;

                    fn expecting(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                        formatter.write_str("a Serde value")
                    }

                    fn visit_unit<E>(self) -> #{Result}<SerdeDeserializeValue, E> { #{Ok}(SerdeDeserializeValue::Null) }
                    fn visit_none<E>(self) -> #{Result}<SerdeDeserializeValue, E> { #{Ok}(SerdeDeserializeValue::Null) }
                    fn visit_bool<E>(self, value: bool) -> #{Result}<SerdeDeserializeValue, E> { #{Ok}(SerdeDeserializeValue::Bool(value)) }
                    fn visit_i8<E>(self, value: i8) -> #{Result}<SerdeDeserializeValue, E> { #{Ok}(SerdeDeserializeValue::I64(value as i64)) }
                    fn visit_i16<E>(self, value: i16) -> #{Result}<SerdeDeserializeValue, E> { #{Ok}(SerdeDeserializeValue::I64(value as i64)) }
                    fn visit_i32<E>(self, value: i32) -> #{Result}<SerdeDeserializeValue, E> { #{Ok}(SerdeDeserializeValue::I64(value as i64)) }
                    fn visit_i64<E>(self, value: i64) -> #{Result}<SerdeDeserializeValue, E> { #{Ok}(SerdeDeserializeValue::I64(value)) }
                    fn visit_u8<E>(self, value: u8) -> #{Result}<SerdeDeserializeValue, E> { #{Ok}(SerdeDeserializeValue::U64(value as u64)) }
                    fn visit_u16<E>(self, value: u16) -> #{Result}<SerdeDeserializeValue, E> { #{Ok}(SerdeDeserializeValue::U64(value as u64)) }
                    fn visit_u32<E>(self, value: u32) -> #{Result}<SerdeDeserializeValue, E> { #{Ok}(SerdeDeserializeValue::U64(value as u64)) }
                    fn visit_u64<E>(self, value: u64) -> #{Result}<SerdeDeserializeValue, E> { #{Ok}(SerdeDeserializeValue::U64(value)) }
                    fn visit_f32<E>(self, value: f32) -> #{Result}<SerdeDeserializeValue, E> { #{Ok}(SerdeDeserializeValue::F64(value as f64)) }
                    fn visit_f64<E>(self, value: f64) -> #{Result}<SerdeDeserializeValue, E> { #{Ok}(SerdeDeserializeValue::F64(value)) }

                    fn visit_str<E>(self, value: &str) -> #{Result}<SerdeDeserializeValue, E>
                    where
                        E: #{serde}::de::Error,
                    {
                        #{Ok}(SerdeDeserializeValue::String(value.to_string()))
                    }

                    fn visit_string<E>(self, value: #{String}) -> #{Result}<SerdeDeserializeValue, E> {
                        #{Ok}(SerdeDeserializeValue::String(value))
                    }

                    fn visit_bytes<E>(self, value: &[u8]) -> #{Result}<SerdeDeserializeValue, E> {
                        #{Ok}(SerdeDeserializeValue::Bytes(value.to_vec()))
                    }

                    fn visit_byte_buf<E>(self, value: #{Vec}<u8>) -> #{Result}<SerdeDeserializeValue, E> {
                        #{Ok}(SerdeDeserializeValue::Bytes(value))
                    }

                    fn visit_some<D>(self, deserializer: D) -> #{Result}<SerdeDeserializeValue, D::Error>
                    where
                        D: #{serde}::Deserializer<'de>,
                    {
                        <SerdeDeserializeValue as #{serde}::Deserialize>::deserialize(deserializer)
                    }

                    fn visit_newtype_struct<D>(self, deserializer: D) -> #{Result}<SerdeDeserializeValue, D::Error>
                    where
                        D: #{serde}::Deserializer<'de>,
                    {
                        <SerdeDeserializeValue as #{serde}::Deserialize>::deserialize(deserializer)
                    }

                    fn visit_seq<A>(self, mut seq: A) -> #{Result}<SerdeDeserializeValue, A::Error>
                    where
                        A: #{serde}::de::SeqAccess<'de>,
                    {
                        let mut values = #{Vec}::with_capacity(seq.size_hint().unwrap_or(0));
                        while let #{Some}(value) = seq.next_element()? {
                            values.push(value);
                        }
                        #{Ok}(SerdeDeserializeValue::Seq(values))
                    }

                    fn visit_map<A>(self, mut map: A) -> #{Result}<SerdeDeserializeValue, A::Error>
                    where
                        A: #{serde}::de::MapAccess<'de>,
                    {
                        let mut entries = #{Vec}::with_capacity(map.size_hint().unwrap_or(0));
                        while let #{Some}(entry) = map.next_entry()? {
                            entries.push(entry);
                        }
                        #{Ok}(SerdeDeserializeValue::Map(entries))
                    }
                }

                impl<'de> #{serde}::Deserialize<'de> for SerdeDeserializeValue {
                    fn deserialize<D>(deserializer: D) -> #{Result}<Self, D::Error>
                    where
                        D: #{serde}::Deserializer<'de>,
                    {
                        deserializer.deserialize_any(SerdeDeserializeValueVisitor)
                    }
                }
                """,
                *SupportStructures.codegenScope,
                *RuntimeType.preludeScope,
            )
        }

    companion object {
        private val DeserializerModule = RustModule.pubCrate("de", parent = SerdeModule)
    }
}
