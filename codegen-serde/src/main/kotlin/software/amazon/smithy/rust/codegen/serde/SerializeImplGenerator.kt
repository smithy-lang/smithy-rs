/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.serde

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.knowledge.TopDownIndex
import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.shapes.BooleanShape
import software.amazon.smithy.model.shapes.CollectionShape
import software.amazon.smithy.model.shapes.DocumentShape
import software.amazon.smithy.model.shapes.DoubleShape
import software.amazon.smithy.model.shapes.FloatShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.NumberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.TimestampShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.SensitiveTrait
import software.amazon.smithy.model.traits.SparseTrait
import software.amazon.smithy.model.traits.StreamingTrait
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.join
import software.amazon.smithy.rust.codegen.core.rustlang.map
import software.amazon.smithy.rust.codegen.core.rustlang.plus
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.withBlock
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.SimpleShapes
import software.amazon.smithy.rust.codegen.core.smithy.contextName
import software.amazon.smithy.rust.codegen.core.smithy.generators.UnionGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.renderUnknownVariant
import software.amazon.smithy.rust.codegen.core.smithy.isOptional
import software.amazon.smithy.rust.codegen.core.smithy.protocols.shapeFunctionName
import software.amazon.smithy.rust.codegen.core.smithy.protocols.shapeModuleName
import software.amazon.smithy.rust.codegen.core.smithy.rustType
import software.amazon.smithy.rust.codegen.core.smithy.symbolBuilder
import software.amazon.smithy.rust.codegen.core.util.PANIC
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.isEventStream
import software.amazon.smithy.rust.codegen.core.util.isTargetUnit
import software.amazon.smithy.rust.codegen.core.util.letIf
import software.amazon.smithy.rust.codegen.core.util.toPascalCase
import software.amazon.smithy.rust.codegen.server.smithy.ServerRustSettings
import software.amazon.smithy.rust.codegen.server.smithy.hasConstraintTrait

class SerializeImplGenerator(private val codegenContext: CodegenContext) {
    private val model = codegenContext.model
    private val topIndex = TopDownIndex.of(model)

    fun generateRootSerializerForShape(shape: Shape): Writable = serializerFn(shape, null)

    /**
     * Generates a serializer for a given shape. Collection serializer require using a wrapper structure. To handle this,
     * `applyTo` allows passing in a `Writable` referring to an input. This returns a writable that wraps the input if necessary,
     * e.g.
     *
     * `&self.map_shape` => `SerializeSomeSpecificMap(&self.map_shape)`
     *
     * Even if no wrapping is required, the writable that is returned includes the required serializer as dependencies.
     */
    private fun serializerFn(
        shape: Shape,
        applyTo: Writable?,
    ): Writable {
        if (shape is ServiceShape) {
            return topIndex.getContainedOperations(shape).map {
                serializerFn(it, null)
            }.join("\n")
        } else if (shape is OperationShape) {
            if (shape.isEventStream(model)) {
                // Don't generate serializers for event streams
                return writable { }
            }
            return writable {
                serializerFn(model.expectShape(shape.inputShape), null)(this)
                serializerFn(model.expectShape(shape.outputShape), null)(this)
            }
        }
        val name = codegenContext.symbolProvider.shapeFunctionName(codegenContext.serviceShape, shape) + "_serde"
        val deps =
            when (shape) {
                is StructureShape -> RuntimeType.forInlineFun(name, serdeSubmodule(shape), structSerdeImpl(shape))
                is UnionShape -> RuntimeType.forInlineFun(name, serdeSubmodule(shape), serializeUnionImpl(shape))
                is TimestampShape -> serializeDateTime(shape)
                is BlobShape ->
                    if (shape.hasTrait<StreamingTrait>()) {
                        serializeByteStream(shape)
                    } else {
                        serializeBlob(shape)
                    }

                is NumberShape -> serializeNumber(shape)
                is StringShape, is BooleanShape -> directSerde(shape)
                is DocumentShape -> serializeDocument(shape)
                else -> null
            }

        return writable {
            val wrapper =
                when {
                    deps != null -> null
                    shape is MapShape -> serializeMap(shape)
                    shape is CollectionShape -> serializeList(shape)
                    // Need to figure out the best default here.
                    else -> PANIC("No serializer supported for $shape")
                }
            if (wrapper != null && applyTo != null) {
                rustTemplate(
                    "&#{wrapper}(#{applyTo}#{unwrapConstraints})", "wrapper" to wrapper,
                    "applyTo" to applyTo,
                    "unwrapConstraints" to shape.unwrapConstraints(),
                )
            } else {
                deps?.toSymbol().also { addDependency(it) }
                applyTo?.invoke(this)
                shape.unwrapConstraints()(this)
            }
        }
    }

    private fun serializeMap(shape: MapShape): RuntimeType =
        serializeWithWrapper(shape) { value ->
            val member = serializeMember(shape.value, "v")
            val writeEntry =
                writable {
                    when (shape.hasTrait<SparseTrait>()) {
                        true ->
                            rust(
                                """
                                match v {
                                    Some(v) => map.serialize_entry(k.as_str(), &#T)?,
                                    None => map.serialize_entry(k, &None::<usize>)?
                                };
                                """,
                                member,
                            )

                        false -> rust("map.serialize_entry(k.as_str(), &#T)?;", member)
                    }
                }
            writable {
                rustTemplate(
                    """
                    use #{serde}::ser::SerializeMap;
                    let mut map = serializer.serialize_map(Some(#{value}.len()))?;
                    for (k, v) in #{value}.iter() {
                        #{writeEntry}
                    }
                    map.end()
                    """,
                    *SupportStructures.codegenScope,
                    "value" to value,
                    "writeEntry" to writeEntry,
                )
            }
        }

    private fun serializeList(shape: CollectionShape): RuntimeType =
        serializeWithWrapper(shape) { value ->
            val member = serializeMember(shape.member, "v")
            val serializeElement =
                writable {
                    when (shape.hasTrait<SparseTrait>()) {
                        false -> rust("seq.serialize_element(&#T)?;", member)
                        true ->
                            rust(
                                "match v { Some(v) => seq.serialize_element(&#T)?, None => seq.serialize_element(&None::<usize>)? };",
                                member,
                            )
                    }
                }
            writable {
                rustTemplate(
                    """
                    use #{serde}::ser::SerializeSeq;
                    let mut seq = serializer.serialize_seq(Some(#{value}.len()))?;
                    for v in #{value}.iter() {
                        #{element}
                    }
                    seq.end()
                    """,
                    *SupportStructures.codegenScope,
                    "element" to serializeElement,
                    "value" to value,
                )
            }
        }

    private fun serdeSubmodule(shape: Shape) =
        RustModule.pubCrate(
            codegenContext.symbolProvider.shapeModuleName(codegenContext.serviceShape, shape),
            parent = SerdeModule,
        )

    /**
     * Serialize a type that already implements `Serialize` directly via `value.serialize(serializer)`
     * For enums, it adds `as_str()` to convert it into a string directly.
     */
    private fun serializeNumber(shape: NumberShape): RuntimeType {
        val numericType = SimpleShapes.getValue(shape::class)
        return when (shape) {
            is FloatShape, is DoubleShape -> serializeFloat(shape)
            else ->
                RuntimeType.forInlineFun(
                    numericType.toString(),
                    PrimitiveShapesModule,
                ) {
                    implSerializeConfigured(symbolBuilder(shape, numericType).build()) {
                        rustTemplate("self.value.serialize(serializer)")
                    }
                }
        }
    }

    private fun serializeFloat(shape: NumberShape): RuntimeType {
        val numericType = SimpleShapes.getValue(shape::class)
        return RuntimeType.forInlineFun(
            numericType.toString(),
            PrimitiveShapesModule,
        ) {
            implSerializeConfigured(symbolBuilder(shape, numericType).build()) {
                rustTemplate(
                    """
                    if !self.settings.out_of_range_floats_as_strings {
                        return self.value.serialize(serializer)
                    }
                    if self.value.is_nan() {
                        serializer.serialize_str("NaN")
                    } else if *self.value == #{ty}::INFINITY {
                        serializer.serialize_str("Infinity")
                    } else if *self.value == #{ty}::NEG_INFINITY {
                        serializer.serialize_str("-Infinity")
                    } else {
                        self.value.serialize(serializer)
                    }
                    """,
                    "ty" to numericType,
                )
            }
        }
    }

    /**
     * Serialize a type that already implements `Serialize` directly via `value.serialize(serializer)`
     * For enums, it adds `as_str()` to convert it into a string directly.
     */
    private fun directSerde(shape: Shape): RuntimeType {
        return RuntimeType.forInlineFun(
            codegenContext.symbolProvider.toSymbol(shape).rustType().toString(),
            PrimitiveShapesModule,
        ) {
            implSerializeConfigured(codegenContext.symbolProvider.toSymbol(shape)) {
                val baseValue =
                    writable {
                        rust("self.value")
                        shape.unwrapConstraints()(this)
                        if (shape.isStringShape) {
                            rust(".as_str()")
                        }
                    }
                rustTemplate("#{base}.serialize(serializer)", "base" to baseValue)
            }
        }
    }

    /**
     * Serialize a shape by first generating a wrapper struct:
     * ```rust
     * struct WrapperType<'a>(&'a Type);
     * ```
     *
     * Then implementing `Serialize` for `ConfigurableSerdeRef<'a WrapperType>`
     *
     * This exists to allow differing implementations for same-shaped-rust types. For example, consider
     * the following Smithy model:
     *
     * ```smithy
     * list SensitiveList {
     *   member: SensitiveString
     * }
     *
     * list StringList {
     *   member: String
     * }
     * ```
     *
     * These both are `Vec<String>` but must be serialized differently.
     */
    private fun serializeWithWrapper(
        shape: Shape,
        body: (Writable) -> Writable,
    ): RuntimeType {
        val name =
            "Serializer" +
                codegenContext.symbolProvider.shapeFunctionName(codegenContext.serviceShape, shape)
                    .toPascalCase()
        // awkward hack to recover the symbol referring to the type
        val module = serdeSubmodule(shape)
        val type = module.toType().resolve(name).toSymbol()
        val base =
            writable { rust("self.value.0") }.letIf(shape.hasConstraintTrait() && constraintTraitsEnabled()) {
                it.plus { rust(".0") }
            }
        val serialization =
            implSerializeConfiguredWrapper(type) {
                body(base)(this)
            }
        val wrapperStruct =
            RuntimeType.forInlineFun(name, module) {
                rustTemplate(
                    """
                    pub(crate) struct $name<'a>(pub(crate) &'a #{Shape});
                    #{serializer}
                    """,
                    "Shape" to codegenContext.symbolProvider.toSymbol(shape),
                    "serializer" to serialization,
                )
            }
        return wrapperStruct
    }

    private fun constraintTraitsEnabled(): Boolean =
        codegenContext.target == CodegenTarget.SERVER &&
            (codegenContext.settings as ServerRustSettings).codegenConfig.publicConstrainedTypes

    private fun Shape.unwrapConstraints(): Writable {
        val shape = this
        return writable {
            if (constraintTraitsEnabled() && hasConstraintTrait()) {
                if (isBlobShape || isTimestampShape || isDocumentShape || shape is NumberShape) {
                    rust(".0")
                }
            }
        }
    }

    /**
     * Serialize the field of a structure, union, list or map.
     *
     * All actual serialization MUST go through this path as it handles applying the `Sensitive` wrapper.
     */
    private fun serializeMember(
        shape: MemberShape,
        memberRef: String,
    ): Writable {
        val target = codegenContext.model.expectShape(shape.target)
        return writable {
            serializerFn(target) {
                rust("$memberRef")
            }.plus { rust(".serialize_ref(self.settings)") }(this)
        }.letIf(target.hasTrait<SensitiveTrait>()) { memberSerialization ->
            memberSerialization.map {
                rustTemplate(
                    "&#{Sensitive}(#{it}).serialize_ref(self.settings)",
                    *SupportStructures.codegenScope,
                    "it" to it,
                )
            }
        }
    }

    private fun structSerdeImpl(shape: StructureShape): Writable {
        return writable {
            implSerializeConfigured(codegenContext.symbolProvider.toSymbol(shape)) {
                rustTemplate(
                    """
                    use #{serde}::ser::SerializeStruct;
                    """,
                    *SupportStructures.codegenScope,
                )
                Attribute.AllowUnusedMut.render(this)
                rust(
                    "let mut s = serializer.serialize_struct(${
                        shape.contextName(codegenContext.serviceShape).dq()
                    }, ${shape.members().size})?;",
                )
                if (!shape.members().isEmpty()) {
                    rust("let inner = &self.value;")
                    for (member in shape.members()) {
                        val serializedName = member.memberName.dq()
                        val fieldName = codegenContext.symbolProvider.toMemberName(member)
                        val field = safeName("member")
                        val fieldSerialization =
                            writable {
                                rustTemplate(
                                    "s.serialize_field($serializedName, &#{member})?;",
                                    "member" to serializeMember(member, field),
                                )
                            }
                        if (codegenContext.symbolProvider.toSymbol(member).isOptional()) {
                            rust("if let Some($field) = &inner.$fieldName { #T }", fieldSerialization)
                        } else {
                            rust("let $field = &inner.$fieldName; #T", fieldSerialization)
                        }
                    }
                }
                rust("s.end()")
            }
        }
    }

    private fun serializeUnionImpl(shape: UnionShape): Writable {
        val unionName = shape.contextName(codegenContext.serviceShape)
        val symbolProvider = codegenContext.symbolProvider
        val unionSymbol = symbolProvider.toSymbol(shape)

        return writable {
            implSerializeConfigured(unionSymbol) {
                rustBlock("match self.value") {
                    shape.members().forEachIndexed { index, member ->
                        val fieldName = member.memberName.dq()
                        val variantName =
                            if (member.isTargetUnit()) {
                                symbolProvider.toMemberName(member)
                            } else {
                                "${symbolProvider.toMemberName(member)}(inner)"
                            }
                        withBlock("#T::$variantName => {", "},", symbolProvider.toSymbol(shape)) {
                            when (member.isTargetUnit()) {
                                true -> rust("serializer.serialize_unit_variant(${unionName.dq()}, $index, $fieldName)")
                                false ->
                                    rustTemplate(
                                        "serializer.serialize_newtype_variant(${unionName.dq()}, $index, $fieldName, &#{member})",
                                        "member" to serializeMember(member, "inner"),
                                    )
                            }
                        }
                    }
                    if (codegenContext.target.renderUnknownVariant()) {
                        rustTemplate(
                            "#{Union}::${UnionGenerator.UNKNOWN_VARIANT_NAME} => serializer.serialize_str(\"unknown variant!\")",
                            "Union" to unionSymbol,
                        )
                    }
                }
            }
        }
    }

    private fun serializeDateTime(shape: TimestampShape): RuntimeType =
        RuntimeType.forInlineFun("SerializeDateTime", Companion.PrimitiveShapesModule) {
            implSerializeConfigured(codegenContext.symbolProvider.toSymbol(shape)) {
                rust("serializer.serialize_str(&self.value.to_string())")
            }
        }

    private fun serializeBlob(shape: BlobShape): RuntimeType =
        RuntimeType.forInlineFun("SerializeBlob", PrimitiveShapesModule) {
            implSerializeConfigured(RuntimeType.blob(codegenContext.runtimeConfig).toSymbol()) {
                rustTemplate(
                    """
                    if serializer.is_human_readable() {
                        serializer.serialize_str(&#{base64_encode}(self.value.as_ref()))
                    } else {
                        serializer.serialize_bytes(self.value.as_ref())
                    }
                    """,
                    "base64_encode" to RuntimeType.base64Encode(codegenContext.runtimeConfig),
                )
            }
        }

    private fun serializeByteStream(shape: BlobShape): RuntimeType =
        RuntimeType.forInlineFun("SerializeByteStream", PrimitiveShapesModule) {
            implSerializeConfigured(RuntimeType.byteStream(codegenContext.runtimeConfig).toSymbol()) {
                // This doesn't work yet—there is no way to get data out of a ByteStream from a sync context
                rustTemplate(
                    """
                    let Some(bytes) = self.value.bytes() else {
                        return serializer.serialize_str("streaming data")
                    };
                    if serializer.is_human_readable() {
                        serializer.serialize_str(&#{base64_encode}(bytes))
                    } else {
                        serializer.serialize_bytes(bytes)
                    }
                    """,
                    "base64_encode" to RuntimeType.base64Encode(codegenContext.runtimeConfig),
                )
            }
        }

    private fun serializeDocument(shape: DocumentShape): RuntimeType =
        RuntimeType.forInlineFun("SerializeDocument", PrimitiveShapesModule) {
            implSerializeConfigured(codegenContext.symbolProvider.toSymbol(shape)) {
                rustTemplate(
                    """
                    match self.value {
                         #{Document}::String(v) => serializer.serialize_str(v),
                         #{Document}::Object(v) => {
                             use #{serde}::ser::SerializeMap;
                             let mut map = serializer.serialize_map(Some(v.len()))?;
                             for (k, v) in v {
                                 map.serialize_entry(k, &v.serialize_ref(self.settings))?;
                             }
                             map.end()
                         },
                         #{Document}::Array(v) => {
                             use #{serde}::ser::SerializeSeq;
                             let mut seq = serializer.serialize_seq(Some(v.len()))?;
                             for e in v {
                                 seq.serialize_element(&e.serialize_ref(self.settings))?;
                             }
                             seq.end()
                         },
                         #{Document}::Number(#{Number}::Float(value)) => value.serialize(serializer),
                         #{Document}::Number(#{Number}::PosInt(value)) => {
                             value.serialize(serializer)
                         },
                         #{Document}::Number(#{Number}::NegInt(value)) => {
                             value.serialize(serializer)
                         },
                         #{Document}::Bool(b) => b.serialize(serializer),
                         #{Document}::Null => serializer.serialize_none(),
                     }
                    """,
                    *SupportStructures.codegenScope,
                    "Document" to RuntimeType.document(codegenContext.runtimeConfig),
                    "Number" to RuntimeType.smithyTypes(codegenContext.runtimeConfig).resolve("Number"),
                )
            }
        }

    private fun RustWriter.implSerializeConfigured(
        shape: Symbol,
        block: Writable,
    ) {
        rustTemplate(
            """
            impl<'a> #{serde}::Serialize for #{ConfigurableSerdeRef}<'a, #{Shape}> {
                fn serialize<S>(&self, serializer: S) -> #{Result}<S::Ok, S::Error>
                where
                    S: #{serde}::Serializer,
                {
                    ##[allow(unused_imports)]
                    use #{SerializeConfigured};
                    #{body}
                }
            }
            """,
            "Shape" to shape, "body" to block, *SupportStructures.codegenScope,
            *RuntimeType.preludeScope,
        )
    }

    private fun implSerializeConfiguredWrapper(
        shape: Symbol,
        block: Writable,
    ): Writable {
        return writable {
            rustTemplate(
                """
                impl<'a, 'b> #{serde}::Serialize for #{ConfigurableSerdeRef}<'a, #{Shape}<'b>> {
                    fn serialize<S>(&self, serializer: S) -> #{Result}<S::Ok, S::Error>
                    where
                        S: #{serde}::Serializer,
                    {
                        ##[allow(unused_imports)]
                        use #{SerializeConfigured};
                        #{body}
                    }
                }
                """,
                "Shape" to shape, "body" to block, *SupportStructures.codegenScope,
                *RuntimeType.preludeScope,
            )
        }
    }

    companion object {
        private val PrimitiveShapesModule = RustModule.pubCrate("primitives", parent = SerdeModule)
    }
}
