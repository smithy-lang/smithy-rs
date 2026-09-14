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
import software.amazon.smithy.rust.codegen.core.smithy.isOptional
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
 * Each shape is deserialized by a typed visitor. Aggregate visitors pass shape-specific seeds directly to Serde and
 * structures populate their generated builders as fields arrive.
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
                    writable { addDependency(seed(shape).toSymbol()) }
                }

            is StructureShape ->
                if (shape.hasEventStreamMember(model)) {
                    writable { }
                } else {
                    writable { addDependency(seed(shape).toSymbol()) }
                }

            is EnumShape -> writable { addDependency(seed(shape).toSymbol()) }
            is StringShape ->
                if (shape.hasTrait<EnumTrait>()) {
                    writable { addDependency(seed(shape).toSymbol()) }
                } else {
                    writable { }
                }

            else -> writable { }
        }

    private fun seed(shape: Shape): RuntimeType =
        RuntimeType.forInlineFun(seedName(shape), DeserializerModule) {
            renderSeed(shape)(this)
        }

    private fun seedName(shape: Shape): String =
        (
            symbolProvider.shapeFunctionName(codegenContext.serviceShape, shape) +
                "_serde_deserialize_seed"
        ).toPascalCase()

    private fun visitorName(shape: Shape): String = seedName(shape) + "Visitor"

    private fun fieldName(shape: Shape): String = seedName(shape) + "Field"

    private fun fieldVisitorName(shape: Shape): String = fieldName(shape) + "Visitor"

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

    private fun renderSeed(shape: Shape): Writable =
        writable {
            val info = parseInfo(shape)
            renderVisitor(shape, info)(this)
            rustTemplate(
                """
                ##[allow(dead_code)]
                struct ${seedName(shape)}<'a> {
                    settings: &'a #{DeserializationSettings},
                    depth: usize,
                }

                impl<'de> #{serde}::de::DeserializeSeed<'de> for ${seedName(shape)}<'_> {
                    type Value = #{Return};

                    fn deserialize<D>(self, deserializer: D) -> #{Result}<Self::Value, D::Error>
                    where
                        D: #{serde}::Deserializer<'de>,
                    {
                        let depth = self.depth.checked_sub(1).ok_or_else(|| {
                            <D::Error as #{serde}::de::Error>::custom(
                                "maximum deserialization depth exceeded"
                            )
                        })?;
                        #{Dispatch}
                    }
                }
                """,
                "Return" to info.symbol,
                "Dispatch" to deserializeDispatch(shape),
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

    private fun renderVisitor(
        shape: Shape,
        info: ReturnSymbolToParse,
    ): Writable =
        when (shape) {
            is StructureShape -> renderStructureVisitor(shape, info)
            is UnionShape -> renderUnionVisitor(shape, info)
            is EnumShape -> renderEnumVisitor(shape, info)
            is StringShape ->
                if (shape.hasTrait<EnumTrait>()) {
                    renderEnumVisitor(shape, info)
                } else {
                    renderStringVisitor(shape, info)
                }

            is BooleanShape -> renderBooleanVisitor(shape, info)
            is NumberShape -> renderNumberVisitor(shape, info)
            is BlobShape -> renderBlobVisitor(shape, info)
            is TimestampShape -> renderTimestampVisitor(shape, info)
            is DocumentShape -> renderDocumentVisitor(shape, info)
            is CollectionShape -> renderCollectionVisitor(shape, info)
            is MapShape -> renderMapVisitor(shape, info)
            else -> writable { rust("// unsupported shape for deserialization") }
        }

    private fun deserializeDispatch(shape: Shape): Writable =
        writable {
            val visitor =
                writable {
                    rustTemplate(
                        "${visitorName(shape)} { settings: self.settings, depth }",
                    )
                }
            when (shape) {
                is StructureShape ->
                    rustTemplate(
                        """
                        deserializer.deserialize_struct(
                            ${shape.contextName(codegenContext.serviceShape).dq()},
                            &[#{Fields}],
                            #{Visitor},
                        )
                        """,
                        "Fields" to
                            writable {
                                rust(shape.members().joinToString(", ") { it.memberName.dq() })
                            },
                        "Visitor" to visitor,
                    )

                is UnionShape ->
                    rustTemplate(
                        """
                        deserializer.deserialize_enum(
                            ${shape.contextName(codegenContext.serviceShape).dq()},
                            &[#{Variants}],
                            #{Visitor},
                        )
                        """,
                        "Variants" to
                            writable {
                                rust(shape.members().joinToString(", ") { it.memberName.dq() })
                            },
                        "Visitor" to visitor,
                    )

                is EnumShape, is StringShape -> rustTemplate("deserializer.deserialize_string(#{Visitor})", "Visitor" to visitor)
                is BooleanShape -> rustTemplate("deserializer.deserialize_bool(#{Visitor})", "Visitor" to visitor)
                is ByteShape -> rustTemplate("deserializer.deserialize_i8(#{Visitor})", "Visitor" to visitor)
                is ShortShape -> rustTemplate("deserializer.deserialize_i16(#{Visitor})", "Visitor" to visitor)
                is IntegerShape -> rustTemplate("deserializer.deserialize_i32(#{Visitor})", "Visitor" to visitor)
                is LongShape -> rustTemplate("deserializer.deserialize_i64(#{Visitor})", "Visitor" to visitor)
                is FloatShape ->
                    rustTemplate(
                        """
                        if self.settings.allow_non_finite_float_strings {
                            deserializer.deserialize_any(#{Visitor})
                        } else {
                            deserializer.deserialize_f32(#{Visitor})
                        }
                        """,
                        "Visitor" to visitor,
                    )

                is DoubleShape ->
                    rustTemplate(
                        """
                        if self.settings.allow_non_finite_float_strings {
                            deserializer.deserialize_any(#{Visitor})
                        } else {
                            deserializer.deserialize_f64(#{Visitor})
                        }
                        """,
                        "Visitor" to visitor,
                    )

                is BigIntegerShape, is BigDecimalShape ->
                    rustTemplate("deserializer.deserialize_any(#{Visitor})", "Visitor" to visitor)

                is BlobShape ->
                    rustTemplate("deserializer.deserialize_any(#{Visitor})", "Visitor" to visitor)

                is TimestampShape -> rustTemplate("deserializer.deserialize_string(#{Visitor})", "Visitor" to visitor)
                is DocumentShape -> rustTemplate("deserializer.deserialize_any(#{Visitor})", "Visitor" to visitor)
                is CollectionShape -> rustTemplate("deserializer.deserialize_seq(#{Visitor})", "Visitor" to visitor)
                is MapShape -> rustTemplate("deserializer.deserialize_map(#{Visitor})", "Visitor" to visitor)
                else ->
                    rustTemplate(
                        "#{Err}(<D::Error as #{serde}::de::Error>::custom(\"unsupported shape for deserialization\"))",
                        *SupportStructures.codegenScope,
                        *RuntimeType.preludeScope,
                    )
            }
        }

    private fun visitorHeader(
        shape: Shape,
        info: ReturnSymbolToParse,
        expecting: String,
        body: Writable,
    ): Writable =
        writable {
            rustTemplate(
                """
                ##[allow(dead_code)]
                struct ${visitorName(shape)}<'a> {
                    settings: &'a #{DeserializationSettings},
                    depth: usize,
                }

                ##[allow(unused_mut, unused_variables)]
                ##[allow(
                    clippy::match_single_binding,
                    clippy::unnecessary_cast,
                    clippy::unnecessary_fallible_conversions,
                )]
                impl<'de> #{serde}::de::Visitor<'de> for ${visitorName(shape)}<'_> {
                    type Value = #{Return};

                    fn expecting(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                        formatter.write_str(${expecting.dq()})
                    }

                    #{Body}
                }
                """,
                "Return" to info.symbol,
                "Body" to body,
                *SupportStructures.codegenScope,
            )
        }

    private fun renderBooleanVisitor(
        shape: BooleanShape,
        info: ReturnSymbolToParse,
    ): Writable =
        visitorHeader(
            shape,
            info,
            "a boolean",
            writable {
                rustTemplate(
                    """
                    fn visit_bool<E>(self, value: bool) -> #{Result}<Self::Value, E> {
                        #{Ok}(value)
                    }
                    """,
                    *RuntimeType.preludeScope,
                )
            },
        )

    private fun renderStringVisitor(
        shape: StringShape,
        info: ReturnSymbolToParse,
    ): Writable =
        visitorHeader(
            shape,
            info,
            "a string",
            writable {
                rustTemplate(
                    """
                    fn visit_str<E>(self, value: &str) -> #{Result}<Self::Value, E> {
                        #{Ok}(value.to_string())
                    }

                    fn visit_string<E>(self, value: #{String}) -> #{Result}<Self::Value, E> {
                        #{Ok}(value)
                    }
                    """,
                    *RuntimeType.preludeScope,
                )
            },
        )

    private fun renderEnumVisitor(
        shape: Shape,
        info: ReturnSymbolToParse,
    ): Writable =
        visitorHeader(
            shape,
            info,
            "an enum string",
            writable {
                rustTemplate(
                    """
                    fn visit_str<E>(self, value: &str) -> #{Result}<Self::Value, E>
                    where
                        E: #{serde}::de::Error,
                    {
                        self.visit_string(value.to_string())
                    }

                    fn visit_string<E>(self, value: #{String}) -> #{Result}<Self::Value, E>
                    where
                        E: #{serde}::de::Error,
                    {
                        #{Parse}
                    }
                    """,
                    "Parse" to enumFromString(info),
                    *SupportStructures.codegenScope,
                    *RuntimeType.preludeScope,
                )
            },
        )

    private fun enumFromString(info: ReturnSymbolToParse): Writable =
        writable {
            when {
                codegenContext.target == CodegenTarget.SERVER && info.isUnconstrained ->
                    rustTemplate("#{Ok}(value)", *RuntimeType.preludeScope)

                codegenContext.target == CodegenTarget.SERVER ->
                    rustTemplate(
                        "<#{Return} as #{TryFrom}<#{String}>>::try_from(value).map_err(E::custom)",
                        "Return" to info.symbol,
                        *RuntimeType.preludeScope,
                    )

                else ->
                    rustTemplate(
                        "#{Ok}(#{Return}::from(value.as_str()))",
                        "Return" to info.symbol,
                        *RuntimeType.preludeScope,
                    )
            }
        }

    private fun renderNumberVisitor(
        shape: NumberShape,
        info: ReturnSymbolToParse,
    ): Writable =
        when (shape) {
            is FloatShape -> renderFloatVisitor(shape, info, "f32")
            is DoubleShape -> renderFloatVisitor(shape, info, "f64")
            is BigIntegerShape -> renderBigNumberVisitor(shape, info, RuntimeType.bigInteger(codegenContext.runtimeConfig))
            is BigDecimalShape -> renderBigNumberVisitor(shape, info, RuntimeType.bigDecimal(codegenContext.runtimeConfig))
            is ByteShape -> renderIntegerVisitor(shape, info, "i8")
            is ShortShape -> renderIntegerVisitor(shape, info, "i16")
            is IntegerShape -> renderIntegerVisitor(shape, info, "i32")
            is LongShape -> renderIntegerVisitor(shape, info, "i64")
            else -> writable { rust("// unsupported number shape") }
        }

    private fun renderIntegerVisitor(
        shape: NumberShape,
        info: ReturnSymbolToParse,
        rustType: String,
    ): Writable =
        visitorHeader(
            shape,
            info,
            "an integer",
            writable {
                listOf("i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64").forEach { source ->
                    rustTemplate(
                        """
                        fn visit_$source<E>(self, value: $source) -> #{Result}<Self::Value, E>
                        where
                            E: #{serde}::de::Error,
                        {
                            <$rustType as #{TryFrom}<$source>>::try_from(value)
                                .map_err(|_| E::custom("integer is out of range"))
                        }
                        """,
                        *SupportStructures.codegenScope,
                        *RuntimeType.preludeScope,
                    )
                }
            },
        )

    private fun renderFloatVisitor(
        shape: NumberShape,
        info: ReturnSymbolToParse,
        rustType: String,
    ): Writable =
        visitorHeader(
            shape,
            info,
            "a floating-point number",
            writable {
                listOf("i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64", "f32", "f64").forEach { source ->
                    rustTemplate(
                        """
                        fn visit_$source<E>(self, value: $source) -> #{Result}<Self::Value, E> {
                            #{Ok}(value as $rustType)
                        }
                        """,
                        *RuntimeType.preludeScope,
                    )
                }
                rustTemplate(
                    """
                    fn visit_str<E>(self, value: &str) -> #{Result}<Self::Value, E>
                    where
                        E: #{serde}::de::Error,
                    {
                        if !self.settings.allow_non_finite_float_strings {
                            return #{Err}(E::custom("expected a floating-point number"));
                        }
                        match value {
                            "NaN" => #{Ok}($rustType::NAN),
                            "Infinity" => #{Ok}($rustType::INFINITY),
                            "-Infinity" => #{Ok}($rustType::NEG_INFINITY),
                            _ => #{Err}(E::custom("expected a floating-point number")),
                        }
                    }

                    fn visit_string<E>(self, value: #{String}) -> #{Result}<Self::Value, E>
                    where
                        E: #{serde}::de::Error,
                    {
                        self.visit_str(value.as_str())
                    }
                    """,
                    *SupportStructures.codegenScope,
                    *RuntimeType.preludeScope,
                )
            },
        )

    private fun renderBigNumberVisitor(
        shape: NumberShape,
        info: ReturnSymbolToParse,
        type: RuntimeType,
    ): Writable =
        visitorHeader(
            shape,
            info,
            "a number",
            writable {
                rustTemplate(
                    """
                    fn visit_str<E>(self, value: &str) -> #{Result}<Self::Value, E>
                    where
                        E: #{serde}::de::Error,
                    {
                        <#{Type} as ::std::str::FromStr>::from_str(value).map_err(E::custom)
                    }

                    fn visit_string<E>(self, value: #{String}) -> #{Result}<Self::Value, E>
                    where
                        E: #{serde}::de::Error,
                    {
                        self.visit_str(value.as_str())
                    }
                    """,
                    "Type" to type,
                    *SupportStructures.codegenScope,
                    *RuntimeType.preludeScope,
                )
                listOf("i64", "u64", "f64").forEach { source ->
                    rustTemplate(
                        """
                        fn visit_$source<E>(self, value: $source) -> #{Result}<Self::Value, E>
                        where
                            E: #{serde}::de::Error,
                        {
                            self.visit_string(value.to_string())
                        }
                        """,
                        *SupportStructures.codegenScope,
                        *RuntimeType.preludeScope,
                    )
                }
            },
        )

    private fun renderBlobVisitor(
        shape: BlobShape,
        info: ReturnSymbolToParse,
    ): Writable =
        writable {
            visitorHeader(
                shape,
                info,
                "a base64 string or bytes",
                writable {
                    rustTemplate(
                        """
                        fn visit_str<E>(self, value: &str) -> #{Result}<Self::Value, E>
                        where
                            E: #{serde}::de::Error,
                        {
                            if value == "streaming data" {
                                return #{Err}(E::custom(
                                    "the streaming data placeholder cannot be deserialized"
                                ));
                            }
                            let bytes = #{base64_decode}(value)
                                .map_err(|_| E::custom("invalid base64 blob"))?;
                            #{Finish}
                        }

                        fn visit_string<E>(self, value: #{String}) -> #{Result}<Self::Value, E>
                        where
                            E: #{serde}::de::Error,
                        {
                            self.visit_str(value.as_str())
                        }

                        fn visit_bytes<E>(self, value: &[u8]) -> #{Result}<Self::Value, E> {
                            let bytes = value.to_vec();
                            #{Finish}
                        }

                        fn visit_byte_buf<E>(self, bytes: #{Vec}<u8>) -> #{Result}<Self::Value, E> {
                            #{Finish}
                        }
                        """,
                        "base64_decode" to RuntimeType.base64Decode(codegenContext.runtimeConfig),
                        "Finish" to finishBlob(shape),
                        *SupportStructures.codegenScope,
                        *RuntimeType.preludeScope,
                    )
                },
            )(this)
        }

    private fun finishBlob(shape: BlobShape): Writable =
        writable {
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

    private fun renderTimestampVisitor(
        shape: TimestampShape,
        info: ReturnSymbolToParse,
    ): Writable =
        visitorHeader(
            shape,
            info,
            "a timestamp string",
            writable {
                rustTemplate(
                    """
                    fn visit_str<E>(self, value: &str) -> #{Result}<Self::Value, E>
                    where
                        E: #{serde}::de::Error,
                    {
                        #{DateTime}::from_str(value, #{Format}::DateTime).map_err(E::custom)
                    }

                    fn visit_string<E>(self, value: #{String}) -> #{Result}<Self::Value, E>
                    where
                        E: #{serde}::de::Error,
                    {
                        self.visit_str(value.as_str())
                    }
                    """,
                    "DateTime" to RuntimeType.dateTime(codegenContext.runtimeConfig),
                    "Format" to RuntimeType.smithyTypes(codegenContext.runtimeConfig).resolve("date_time::Format"),
                    *SupportStructures.codegenScope,
                    *RuntimeType.preludeScope,
                )
            },
        )

    private fun renderDocumentVisitor(
        shape: DocumentShape,
        info: ReturnSymbolToParse,
    ): Writable =
        visitorHeader(
            shape,
            info,
            "a document",
            writable {
                rustTemplate(
                    """
                    fn visit_unit<E>(self) -> #{Result}<Self::Value, E> {
                        #{Ok}(#{Document}::Null)
                    }

                    fn visit_none<E>(self) -> #{Result}<Self::Value, E> {
                        #{Ok}(#{Document}::Null)
                    }

                    fn visit_bool<E>(self, value: bool) -> #{Result}<Self::Value, E> {
                        #{Ok}(#{Document}::Bool(value))
                    }

                    fn visit_str<E>(self, value: &str) -> #{Result}<Self::Value, E> {
                        #{Ok}(#{Document}::String(value.to_string()))
                    }

                    fn visit_string<E>(self, value: #{String}) -> #{Result}<Self::Value, E> {
                        #{Ok}(#{Document}::String(value))
                    }

                    fn visit_i64<E>(self, value: i64) -> #{Result}<Self::Value, E> {
                        if value < 0 {
                            #{Ok}(#{Document}::Number(#{Number}::NegInt(value)))
                        } else {
                            #{Ok}(#{Document}::Number(#{Number}::PosInt(value as u64)))
                        }
                    }

                    fn visit_u64<E>(self, value: u64) -> #{Result}<Self::Value, E> {
                        #{Ok}(#{Document}::Number(#{Number}::PosInt(value)))
                    }

                    fn visit_f64<E>(self, value: f64) -> #{Result}<Self::Value, E> {
                        #{Ok}(#{Document}::Number(#{Number}::Float(value)))
                    }

                    fn visit_some<D>(self, deserializer: D) -> #{Result}<Self::Value, D::Error>
                    where
                        D: #{serde}::Deserializer<'de>,
                    {
                        #{serde}::de::DeserializeSeed::deserialize(
                            ${seedName(shape)} {
                                settings: self.settings,
                                depth: self.depth,
                            },
                            deserializer,
                        )
                    }

                    fn visit_seq<A>(self, mut seq: A) -> #{Result}<Self::Value, A::Error>
                    where
                        A: #{serde}::de::SeqAccess<'de>,
                    {
                        let mut result = #{Vec}::with_capacity(
                            seq.size_hint().unwrap_or(0).min(10_000)
                        );
                        while let #{Some}(value) = seq.next_element_seed(
                            ${seedName(shape)} {
                                settings: self.settings,
                                depth: self.depth,
                            }
                        )? {
                            result.push(value);
                        }
                        #{Ok}(#{Document}::Array(result))
                    }

                    fn visit_map<A>(self, mut map: A) -> #{Result}<Self::Value, A::Error>
                    where
                        A: #{serde}::de::MapAccess<'de>,
                    {
                        // `DocumentObject` rather than a `HashMap` intermediate: a document
                        // parsed from the wire must iterate in the order its entries appeared
                        // in the source data, and a `HashMap` would discard that order.
                        let mut result = #{DocumentObject}::with_capacity(
                            map.size_hint().unwrap_or(0).min(10_000)
                        );
                        while let #{Some}(key) = map.next_key::<#{String}>()? {
                            let value = map.next_value_seed(
                                ${seedName(shape)} {
                                    settings: self.settings,
                                    depth: self.depth,
                                }
                            )?;
                            result.insert(key, value);
                        }
                        #{Ok}(#{Document}::Object(result))
                    }
                    """,
                    "Document" to RuntimeType.document(codegenContext.runtimeConfig),
                    "DocumentObject" to RuntimeType.documentObject(codegenContext.runtimeConfig),
                    "Number" to RuntimeType.smithyTypes(codegenContext.runtimeConfig).resolve("Number"),
                    *SupportStructures.codegenScope,
                    *RuntimeType.preludeScope,
                )
            },
        )

    private fun renderCollectionVisitor(
        shape: CollectionShape,
        info: ReturnSymbolToParse,
    ): Writable =
        visitorHeader(
            shape,
            info,
            "a sequence",
            writable {
                val memberSeed = seed(model.expectShape(shape.member.target))
                rustTemplate(
                    """
                    fn visit_seq<A>(self, mut seq: A) -> #{Result}<Self::Value, A::Error>
                    where
                        A: #{serde}::de::SeqAccess<'de>,
                    {
                        let mut result = #{Vec}::with_capacity(
                            seq.size_hint().unwrap_or(0).min(10_000)
                        );
                        while let #{Some}(value) = seq.next_element_seed(#{MemberSeed})? {
                            result.push(value);
                        }
                        #{Finish}
                    }
                    """,
                    "MemberSeed" to
                        writable {
                            if (shape.hasTrait<SparseTrait>()) {
                                rustTemplate(
                                    """
                                    #{OptionalSeed}(#{Seed} {
                                        settings: self.settings,
                                        depth: self.depth,
                                    })
                                    """,
                                    "OptionalSeed" to optionalSeed(),
                                    "Seed" to memberSeed,
                                )
                            } else {
                                rustTemplate(
                                    """
                                    #{Seed} {
                                        settings: self.settings,
                                        depth: self.depth,
                                    }
                                    """,
                                    "Seed" to memberSeed,
                                )
                            }
                        },
                    "Finish" to finishContainer(shape, info, "A::Error"),
                    *SupportStructures.codegenScope,
                    *RuntimeType.preludeScope,
                )
            },
        )

    private fun renderMapVisitor(
        shape: MapShape,
        info: ReturnSymbolToParse,
    ): Writable =
        visitorHeader(
            shape,
            info,
            "a map",
            writable {
                val keySeed = seed(model.expectShape(shape.key.target))
                val valueSeed = seed(model.expectShape(shape.value.target))
                rustTemplate(
                    """
                    fn visit_map<A>(self, mut map: A) -> #{Result}<Self::Value, A::Error>
                    where
                        A: #{serde}::de::MapAccess<'de>,
                    {
                        let mut result = #{HashMap}::with_capacity(
                            map.size_hint().unwrap_or(0).min(10_000)
                        );
                        while let #{Some}(key) = map.next_key_seed(
                            #{KeySeed} {
                                settings: self.settings,
                                depth: self.depth,
                            }
                        )? {
                            let value = map.next_value_seed(#{ValueSeed})?;
                            if result.insert(key, value).is_some() {
                                return #{Err}(<A::Error as #{serde}::de::Error>::custom(
                                    "duplicate map key"
                                ));
                            }
                        }
                        #{Finish}
                    }
                    """,
                    "HashMap" to RuntimeType.HashMap,
                    "KeySeed" to keySeed,
                    "ValueSeed" to
                        writable {
                            if (shape.hasTrait<SparseTrait>()) {
                                rustTemplate(
                                    """
                                    #{OptionalSeed}(#{Seed} {
                                        settings: self.settings,
                                        depth: self.depth,
                                    })
                                    """,
                                    "OptionalSeed" to optionalSeed(),
                                    "Seed" to valueSeed,
                                )
                            } else {
                                rustTemplate(
                                    """
                                    #{Seed} {
                                        settings: self.settings,
                                        depth: self.depth,
                                    }
                                    """,
                                    "Seed" to valueSeed,
                                )
                            }
                        },
                    "Finish" to finishContainer(shape, info, "A::Error"),
                    *SupportStructures.codegenScope,
                    *RuntimeType.preludeScope,
                )
            },
        )

    private fun finishContainer(
        shape: Shape,
        info: ReturnSymbolToParse,
        errorType: String,
    ): Writable =
        writable {
            if (info.isUnconstrained) {
                rustTemplate("#{Ok}(#{Return}(result))", "Return" to info.symbol, *RuntimeType.preludeScope)
            } else if (serverContext != null && shape.canReachConstrainedShape(model, symbolProvider)) {
                rustTemplate(
                    """
                    <#{Return} as #{TryFrom}<_>>::try_from(result)
                        .map_err(<$errorType as #{serde}::de::Error>::custom)
                    """,
                    "Return" to info.symbol,
                    *SupportStructures.codegenScope,
                    *RuntimeType.preludeScope,
                )
            } else {
                rustTemplate("#{Ok}(result)", *RuntimeType.preludeScope)
            }
        }

    private fun renderStructureVisitor(
        shape: StructureShape,
        info: ReturnSymbolToParse,
    ): Writable =
        writable {
            renderStructureField(shape)(this)
            val builder = structureBuilderSymbol(shape)
            visitorHeader(
                shape,
                info,
                "a structure",
                writable {
                    rustTemplate(
                        """
                        fn visit_seq<A>(self, mut seq: A) -> #{Result}<Self::Value, A::Error>
                        where
                            A: #{serde}::de::SeqAccess<'de>,
                        {
                            let mut builder = #{Builder}::default();
                            #{SequenceMembers}
                            while seq.next_element::<#{serde}::de::IgnoredAny>()?.is_some() {}
                            #{Finish}
                        }

                        fn visit_map<A>(self, mut map: A) -> #{Result}<Self::Value, A::Error>
                        where
                            A: #{serde}::de::MapAccess<'de>,
                        {
                            let mut builder = #{Builder}::default();
                            let mut seen: #{HashSet}<&'static str> = #{HashSet}::new();
                            while let #{Some}(field) = map.next_key::<${fieldName(shape)}>()? {
                                match field {
                                    #{MapMembers}
                                    ${fieldName(shape)}::Ignore => {
                                        map.next_value::<#{serde}::de::IgnoredAny>()?;
                                    }
                                }
                            }
                            #{Finish}
                        }
                        """,
                        "Builder" to builder,
                        "HashSet" to RuntimeType.std.resolve("collections::HashSet"),
                        "SequenceMembers" to
                            writable {
                                shape.members().forEach { member ->
                                    renderStructureSequenceMember(shape, member)
                                }
                            },
                        "MapMembers" to
                            writable {
                                shape.members().forEachIndexed { index, member ->
                                    renderStructureMapMember(shape, member, index)
                                }
                            },
                        "Finish" to finishStructure(shape, info, "A::Error"),
                        *SupportStructures.codegenScope,
                        *RuntimeType.preludeScope,
                    )
                },
            )(this)
        }

    private fun renderStructureField(shape: StructureShape): Writable =
        writable {
            rustTemplate(
                """
                enum ${fieldName(shape)} {
                    #{Variants}
                    Ignore,
                }

                struct ${fieldVisitorName(shape)};

                ##[allow(clippy::match_single_binding)]
                impl<'de> #{serde}::de::Visitor<'de> for ${fieldVisitorName(shape)} {
                    type Value = ${fieldName(shape)};

                    fn expecting(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                        formatter.write_str("a structure field name")
                    }

                    fn visit_str<E>(self, value: &str) -> #{Result}<Self::Value, E>
                    where
                        E: #{serde}::de::Error,
                    {
                        match value {
                            #{Matches}
                            _ => #{Ok}(${fieldName(shape)}::Ignore),
                        }
                    }
                }

                impl<'de> #{serde}::Deserialize<'de> for ${fieldName(shape)} {
                    fn deserialize<D>(deserializer: D) -> #{Result}<Self, D::Error>
                    where
                        D: #{serde}::Deserializer<'de>,
                    {
                        deserializer.deserialize_identifier(${fieldVisitorName(shape)})
                    }
                }
                """,
                "Variants" to
                    writable {
                        shape.members().indices.forEach { rust("Field$it,") }
                    },
                "Matches" to
                    writable {
                        shape.members().forEachIndexed { index, member ->
                            rust("${member.memberName.dq()} => Ok(${fieldName(shape)}::Field$index),")
                        }
                    },
                *SupportStructures.codegenScope,
                *RuntimeType.preludeScope,
            )
        }

    private fun structureBuilderSymbol(shape: StructureShape): Symbol =
        when {
            serverContext == null -> symbolProvider.symbolForBuilder(shape)
            shape.isReachableFromOperationInput() -> shape.serverBuilderSymbol(serverContext)
            else -> shape.serverBuilderSymbol(symbolProvider, false)
        }

    private fun RustWriter.renderStructureSequenceMember(
        container: StructureShape,
        member: MemberShape,
    ) {
        val target = model.expectShape(member.target)
        val targetInfo = parseInfo(target)
        val field = symbolProvider.toMemberName(member)
        val memberSymbol = symbolProvider.toSymbol(member)
        if (memberSymbol.isOptional()) {
            rustTemplate(
                """
                let parsed = match seq.next_element_seed(
                    #{OptionalSeed}(#{TargetSeed} {
                        settings: self.settings,
                        depth: self.depth,
                    })
                )? {
                    #{Some}(parsed) => parsed,
                    #{None} => return #{Finish},
                };
                if let #{Some}(parsed) = parsed {
                    #{Prepare}
                    builder.$field = #{Some}(parsed);
                }
                """,
                "OptionalSeed" to optionalSeed(),
                "TargetSeed" to seed(target),
                "Finish" to finishStructure(container, parseInfo(container), "A::Error"),
                "Prepare" to
                    prepareMember(
                        container,
                        target,
                        targetInfo,
                        memberSymbol.isRustBoxed(),
                        "A::Error",
                    ),
                *RuntimeType.preludeScope,
            )
            return
        }
        rustTemplate(
            """
            let parsed = match seq.next_element_seed(
                #{TargetSeed} {
                    settings: self.settings,
                    depth: self.depth,
                }
            )? {
                #{Some}(parsed) => parsed,
                #{None} => return #{Finish},
            };
            #{Prepare}
            builder.$field = #{Some}(parsed);
            """,
            "TargetSeed" to seed(target),
            "Finish" to finishStructure(container, parseInfo(container), "A::Error"),
            "Prepare" to
                prepareMember(
                    container,
                    target,
                    targetInfo,
                    memberSymbol.isRustBoxed(),
                    "A::Error",
                ),
            *RuntimeType.preludeScope,
        )
    }

    private fun RustWriter.renderStructureMapMember(
        container: StructureShape,
        member: MemberShape,
        index: Int,
    ) {
        val target = model.expectShape(member.target)
        val targetInfo = parseInfo(target)
        val field = symbolProvider.toMemberName(member)
        val wireName = member.memberName
        val memberSymbol = symbolProvider.toSymbol(member)
        rustTemplate(
            """
            ${fieldName(container)}::Field$index => {
                if !seen.insert(${wireName.dq()}) {
                    return #{Err}(<A::Error as #{serde}::de::Error>::custom(
                        ${"duplicate field `$wireName`".dq()}
                    ));
                }
                #{Parse}
            }
            """,
            "Parse" to
                writable {
                    if (memberSymbol.isOptional()) {
                        rustTemplate(
                            """
                            let parsed = map.next_value_seed(
                                #{OptionalSeed}(#{TargetSeed} {
                                    settings: self.settings,
                                    depth: self.depth,
                                })
                            )?;
                            if let #{Some}(parsed) = parsed {
                                #{Prepare}
                                builder.$field = #{Some}(parsed);
                            }
                            """,
                            "OptionalSeed" to optionalSeed(),
                            "TargetSeed" to seed(target),
                            "Prepare" to
                                prepareMember(
                                    container,
                                    target,
                                    targetInfo,
                                    memberSymbol.isRustBoxed(),
                                    "A::Error",
                                ),
                            *RuntimeType.preludeScope,
                        )
                    } else {
                        rustTemplate(
                            """
                            let parsed = map.next_value_seed(
                                #{TargetSeed} {
                                    settings: self.settings,
                                    depth: self.depth,
                                }
                            )?;
                            #{Prepare}
                            builder.$field = #{Some}(parsed);
                            """,
                            "TargetSeed" to seed(target),
                            "Prepare" to
                                prepareMember(
                                    container,
                                    target,
                                    targetInfo,
                                    memberSymbol.isRustBoxed(),
                                    "A::Error",
                                ),
                            *RuntimeType.preludeScope,
                        )
                    }
                },
            *SupportStructures.codegenScope,
            *RuntimeType.preludeScope,
        )
    }

    private fun prepareMember(
        container: Shape,
        target: Shape,
        targetInfo: ReturnSymbolToParse,
        boxed: Boolean,
        errorType: String,
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
                convertServerOutputMember(target, targetInfo, errorType)(this)
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
        errorType: String,
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
                        .map_err(<$errorType as #{serde}::de::Error>::custom)?;
                    let parsed: #{Final} = parsed.into();
                    """,
                    "Constrained" to constrainedSymbol,
                    "Parsed" to targetInfo.symbol,
                    "Final" to finalSymbol,
                    *SupportStructures.codegenScope,
                    *RuntimeType.preludeScope,
                )
            } else {
                rustTemplate(
                    """
                    let parsed = <#{Final} as #{TryFrom}<#{Parsed}>>::try_from(parsed)
                        .map_err(<$errorType as #{serde}::de::Error>::custom)?;
                    """,
                    "Final" to finalSymbol,
                    "Parsed" to targetInfo.symbol,
                    *SupportStructures.codegenScope,
                    *RuntimeType.preludeScope,
                )
            }
        }

    private fun finishStructure(
        shape: StructureShape,
        info: ReturnSymbolToParse,
        errorType: String,
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
                rustTemplate(
                    """
                    builder.build().map_err(<$errorType as #{serde}::de::Error>::custom)
                    """,
                    *SupportStructures.codegenScope,
                )
            } else {
                rustTemplate("#{Ok}(builder.build())", *RuntimeType.preludeScope)
            }
        }

    private fun renderUnionVisitor(
        shape: UnionShape,
        info: ReturnSymbolToParse,
    ): Writable =
        writable {
            renderUnionField(shape)(this)
            visitorHeader(
                shape,
                info,
                "an externally tagged union",
                writable {
                    rustTemplate(
                        """
                        fn visit_enum<A>(self, data: A) -> #{Result}<Self::Value, A::Error>
                        where
                            A: #{serde}::de::EnumAccess<'de>,
                        {
                            let (variant, access) = data.variant::<${fieldName(shape)}>()?;
                            match variant {
                                #{Variants}
                            }
                        }
                        """,
                        "Variants" to
                            writable {
                                shape.members().forEachIndexed { index, member ->
                                    renderUnionVariant(shape, member, info, index)
                                }
                            },
                        *SupportStructures.codegenScope,
                        *RuntimeType.preludeScope,
                    )
                },
            )(this)
        }

    private fun renderUnionField(shape: UnionShape): Writable =
        writable {
            rustTemplate(
                """
                enum ${fieldName(shape)} {
                    #{Variants}
                }

                struct ${fieldVisitorName(shape)};

                impl<'de> #{serde}::de::Visitor<'de> for ${fieldVisitorName(shape)} {
                    type Value = ${fieldName(shape)};

                    fn expecting(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                        formatter.write_str("a union variant name")
                    }

                    fn visit_str<E>(self, value: &str) -> #{Result}<Self::Value, E>
                    where
                        E: #{serde}::de::Error,
                    {
                        match value {
                            #{Matches}
                            _ => #{Err}(E::unknown_variant(value, &[#{Names}])),
                        }
                    }

                    fn visit_u64<E>(self, value: u64) -> #{Result}<Self::Value, E>
                    where
                        E: #{serde}::de::Error,
                    {
                        match value {
                            #{IndexMatches}
                            _ => #{Err}(E::invalid_value(
                                #{serde}::de::Unexpected::Unsigned(value),
                                &self,
                            )),
                        }
                    }
                }

                impl<'de> #{serde}::Deserialize<'de> for ${fieldName(shape)} {
                    fn deserialize<D>(deserializer: D) -> #{Result}<Self, D::Error>
                    where
                        D: #{serde}::Deserializer<'de>,
                    {
                        deserializer.deserialize_identifier(${fieldVisitorName(shape)})
                    }
                }
                """,
                "Variants" to
                    writable {
                        shape.members().indices.forEach { rust("Field$it,") }
                    },
                "Matches" to
                    writable {
                        shape.members().forEachIndexed { index, member ->
                            rust("${member.memberName.dq()} => Ok(${fieldName(shape)}::Field$index),")
                        }
                    },
                "Names" to
                    writable {
                        rust(shape.members().joinToString(", ") { it.memberName.dq() })
                    },
                "IndexMatches" to
                    writable {
                        shape.members().indices.forEach { rust("$it => Ok(${fieldName(shape)}::Field$it),") }
                    },
                *SupportStructures.codegenScope,
                *RuntimeType.preludeScope,
            )
        }

    private fun RustWriter.renderUnionVariant(
        container: UnionShape,
        member: MemberShape,
        unionInfo: ReturnSymbolToParse,
        index: Int,
    ) {
        val variant = symbolProvider.toMemberName(member)
        if (member.isTargetUnit()) {
            rustTemplate(
                """
                ${fieldName(container)}::Field$index => {
                    #{serde}::de::VariantAccess::unit_variant(access)?;
                    #{Ok}(#{Return}::$variant)
                }
                """,
                "Return" to unionInfo.symbol,
                *SupportStructures.codegenScope,
                *RuntimeType.preludeScope,
            )
        } else {
            val target = model.expectShape(member.target)
            val targetInfo = parseInfo(target)
            rustTemplate(
                """
                ${fieldName(container)}::Field$index => {
                    let parsed = #{serde}::de::VariantAccess::newtype_variant_seed(
                        access,
                        #{TargetSeed} {
                            settings: self.settings,
                            depth: self.depth,
                        },
                    )?;
                    #{Prepare}
                    #{Ok}(#{Return}::$variant(parsed))
                }
                """,
                "TargetSeed" to seed(target),
                "Prepare" to
                    prepareMember(
                        container,
                        target,
                        targetInfo,
                        member.hasTrait<RustBoxTrait>(),
                        "A::Error",
                    ),
                "Return" to unionInfo.symbol,
                *SupportStructures.codegenScope,
                *RuntimeType.preludeScope,
            )
        }
    }

    private fun RustWriter.renderNominalDeserializeImpl(
        shape: Shape,
        info: ReturnSymbolToParse,
    ) {
        val finalSymbol = symbolProvider.toSymbol(shape)
        rustTemplate(
            """
            impl<'de> #{serde}::Deserialize<'de> for #{Final} {
                fn deserialize<D>(deserializer: D) -> #{Result}<Self, D::Error>
                where
                    D: #{serde}::Deserializer<'de>,
                {
                    let settings = #{DeserializationSettings}::current();
                    let parsed = #{serde}::de::DeserializeSeed::deserialize(
                        ${seedName(shape)} {
                            settings: &settings,
                            depth: 128,
                        },
                        deserializer,
                    )?;
                    #{Finalize}
                }
            }
            """,
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

    private fun optionalSeed(): RuntimeType =
        RuntimeType.forInlineFun("SerdeOptionalSeed", DeserializerModule) {
            rustTemplate(
                """
                struct SerdeOptionalSeed<S>(S);

                struct SerdeOptionalSeedVisitor<S>(#{Option}<S>);

                impl<'de, S> #{serde}::de::Visitor<'de> for SerdeOptionalSeedVisitor<S>
                where
                    S: #{serde}::de::DeserializeSeed<'de>,
                {
                    type Value = #{Option}<S::Value>;

                    fn expecting(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                        formatter.write_str("an optional value")
                    }

                    fn visit_none<E>(self) -> #{Result}<Self::Value, E> {
                        #{Ok}(#{None})
                    }

                    fn visit_unit<E>(self) -> #{Result}<Self::Value, E> {
                        #{Ok}(#{None})
                    }

                    fn visit_some<D>(mut self, deserializer: D) -> #{Result}<Self::Value, D::Error>
                    where
                        D: #{serde}::Deserializer<'de>,
                    {
                        let seed = self.0.take().expect("optional seed is visited once");
                        seed.deserialize(deserializer).map(#{Some})
                    }
                }

                impl<'de, S> #{serde}::de::DeserializeSeed<'de> for SerdeOptionalSeed<S>
                where
                    S: #{serde}::de::DeserializeSeed<'de>,
                {
                    type Value = #{Option}<S::Value>;

                    fn deserialize<D>(self, deserializer: D) -> #{Result}<Self::Value, D::Error>
                    where
                        D: #{serde}::Deserializer<'de>,
                    {
                        deserializer.deserialize_option(SerdeOptionalSeedVisitor(#{Some}(self.0)))
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
