/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.serde

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.neighbor.Walker
import software.amazon.smithy.model.shapes.CollectionShape
import software.amazon.smithy.model.shapes.DocumentShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.NumberShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.TimestampShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.SensitiveTrait
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.DependencyScope
import software.amazon.smithy.rust.codegen.core.rustlang.Feature
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.map
import software.amazon.smithy.rust.codegen.core.rustlang.plus
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.withBlock
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.contextName
import software.amazon.smithy.rust.codegen.core.smithy.generators.UnionGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.renderUnknownVariant
import software.amazon.smithy.rust.codegen.core.smithy.isOptional
import software.amazon.smithy.rust.codegen.core.smithy.protocols.shapeFunctionName
import software.amazon.smithy.rust.codegen.core.smithy.rustType
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.isTargetUnit
import software.amazon.smithy.rust.codegen.core.util.letIf
import software.amazon.smithy.rust.codegen.core.util.toPascalCase

val SerdeFeature = Feature("serde", false, listOf("dep:serde"))
val Module =
    RustModule.public(
        "serde_impl",
        additionalAttributes = listOf(Attribute.featureGate(SerdeFeature.name)),
        documentationOverride = "Implementations of `serde` for model types",
    )

class KotlinClientSerdeDecorator : ClientCodegenDecorator {
    override val name: String = "ClientSerdeDecorator"
    override val order: Byte = 0

    override fun extras(
        codegenContext: ClientCodegenContext,
        rustCrate: RustCrate,
    ) {
        rustCrate.mergeFeature(SerdeFeature)
        val generator = SerializeImplGenerator(codegenContext)
        rustCrate.withModule(Module) {
            serializationRoots(codegenContext).forEach {
                generator.generateRootSerializerForShape(
                    it,
                )(this)
            }
            addDependency(SupportStructures.serializeRedacted().toSymbol())
            addDependency(SupportStructures.serializeUnredacted().toSymbol())
        }
    }
}

/**
 * All entry points for serialization in the service closure.
 */
fun serializationRoots(ctx: CodegenContext): List<Shape> {
    val serviceShape = ctx.serviceShape
    val walker = Walker(ctx.model)
    return walker.walkShapes(serviceShape).filter { it.hasTrait<SerdeTrait>() }
}

class SerializeImplGenerator(private val codegenContext: CodegenContext) {
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
        val name = codegenContext.symbolProvider.shapeFunctionName(codegenContext.serviceShape, shape) + "_serde"
        val deps =
            when (shape) {
                is StructureShape -> RuntimeType.forInlineFun(name, Module, structSerdeImpl(shape))
                is UnionShape -> RuntimeType.forInlineFun(name, Module, serializeUnionImpl(shape))
                is TimestampShape -> serializeDateTime(shape)
                is StringShape, is NumberShape -> directSerde(shape)
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
                    else -> serializeWithTodo(shape)
                }
            if (wrapper != null && applyTo != null) {
                rustTemplate("&#{wrapper}(#{applyTo})", "wrapper" to wrapper, "applyTo" to applyTo)
            } else {
                deps?.toSymbol().also { addDependency(it) }
                applyTo?.invoke(this)
            }
        }
    }

    private fun serializeWithTodo(shape: Shape): RuntimeType =
        serializeWithWrapper(shape) { _ ->
            // PANIC("cant serialize $shape")
            writable {
                rust("serializer.serialize_str(\"todo\")")
            }
        }

    private fun serializeMap(shape: MapShape): RuntimeType =
        serializeWithWrapper(shape) { value ->
            writable {
                rustTemplate(
                    """
                    use #{SerializeConfigured};
                    use #{serde}::ser::SerializeMap;
                    let mut map = serializer.serialize_map(Some(#{value}.len()))?;
                    for (k, v) in self.value.0.iter() {
                        map.serialize_entry(k, #{member})?;
                    }
                    map.end()
                    """,
                    *SupportStructures.codegenScope,
                    "value" to value,
                    "member" to serializeMember(shape.value, "v"),
                )
            }
        }

    private fun serializeList(shape: CollectionShape): RuntimeType =
        serializeWithWrapper(shape) { value ->
            writable {
                rustTemplate(
                    """
                    use #{SerializeConfigured};
                    use #{serde}::ser::SerializeSeq;
                    let mut seq = serializer.serialize_seq(Some(#{value}.len()))?;
                    for v in self.value.0.iter() {
                        seq.serialize_element(#{member})?;
                    }
                    seq.end()
                    """,
                    *SupportStructures.codegenScope,
                    "value" to value,
                    "member" to serializeMember(shape.member, "v"),
                )
            }
        }

    /**
     * Serialize a type that already implements `Serialize` directly via `value.serialize(serializer)`
     */
    private fun directSerde(shape: Shape): RuntimeType {
        return RuntimeType.forInlineFun(codegenContext.symbolProvider.toSymbol(shape).rustType().toString(), Module) {
            implSerializeConfigured(codegenContext.symbolProvider.toSymbol(shape)) {
                val baseValue =
                    writable { rust("self.value") }.letIf(shape.isStringShape) { it.plus(writable(".as_str()")) }
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
        val type = Module.toType().resolve(name).toSymbol()
        val base = writable { rust("self.value.0") }
        val serialization =
            implSerializeConfiguredWrapper(type) {
                body(base)(this)
            }
        val wrapperStruct =
            RuntimeType.forInlineFun(name, Module) {
                rustTemplate(
                    """
                    struct $name<'a>(&'a #{Shape});
                    #{serializer}
                    """,
                    "Shape" to codegenContext.symbolProvider.toSymbol(shape),
                    "serializer" to serialization,
                )
            }
        return wrapperStruct
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
                rust("&$memberRef")
            }.plus { rust(".serialize_ref(&self.settings)") }(this)
        }.letIf(target.hasTrait<SensitiveTrait>()) { memberSerialization ->
            memberSerialization.map {
                rustTemplate(
                    "&#{Sensitive}(#{it}).serialize_ref(&self.settings)",
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
                    use #{SerializeConfigured};
                    """,
                    *SupportStructures.codegenScope,
                )
                rust(
                    "let mut s = serializer.serialize_struct(${
                        shape.contextName(codegenContext.serviceShape).dq()
                    }, ${shape.members().size})?;",
                )
                rust("let inner = &self.value;")
                for (member in shape.members()) {
                    val serializedName = member.memberName.dq()
                    val fieldName = codegenContext.symbolProvider.toMemberName(member)
                    val field = safeName("member")
                    val fieldSerialization =
                        writable {
                            rustTemplate(
                                "s.serialize_field($serializedName, #{member})?;",
                                "member" to serializeMember(member, field),
                            )
                        }
                    if (codegenContext.symbolProvider.toSymbol(member).isOptional()) {
                        rustTemplate(
                            "if let Some($field) = &inner.$fieldName { #{serializeField} }",
                            "serializeField" to fieldSerialization,
                        )
                    } else {
                        rustTemplate(
                            "let $field = &inner.$fieldName; #{serializeField}",
                            "serializeField" to fieldSerialization,
                        )
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
                rustTemplate(
                    """
                    use #{SerializeConfigured};
                    """,
                    *SupportStructures.codegenScope,
                )
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
                            rustTemplate(
                                "serializer.serialize_newtype_variant(${unionName.dq()}, $index, $fieldName, #{member})",
                                "member" to serializeMember(member, "inner"),
                            )
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
        RuntimeType.forInlineFun("SerializeDateTime", Module) {
            implSerializeConfigured(codegenContext.symbolProvider.toSymbol(shape)) {
                rust("serializer.serialize_str(&self.value.to_string())")
            }
        }

    private fun serializeDocument(shape: DocumentShape): RuntimeType =
        RuntimeType.forInlineFun("SerializeDocument", Module) {
            implSerializeConfigured(codegenContext.symbolProvider.toSymbol(shape)) {
                rustTemplate(
                    """
                    use #{SerializeConfigured};
                    match self.value {
                         #{Document}::String(v) => serializer.serialize_str(v),
                         #{Document}::Object(v) => {
                             use #{serde}::ser::SerializeMap;
                             let mut map = serializer.serialize_map(Some(v.len()))?;
                             for (k, v) in v.iter() {
                                 map.serialize_entry(k, &v.serialize_ref(&self.settings))?;
                             }
                             map.end()
                         },
                         #{Document}::Array(v) => {
                             use #{serde}::ser::SerializeSeq;
                             let mut seq = serializer.serialize_seq(Some(v.len()))?;
                             for e in v.iter() {
                                 seq.serialize_element(&e.serialize_ref(&self.settings))?;
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
                fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
                where
                    S: #{serde}::Serializer,
                {
                    #{body}
                }
            }
            """,
            "Shape" to shape, "body" to block, *SupportStructures.codegenScope,
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
                    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
                    where
                        S: #{serde}::Serializer,
                    {
                        #{body}
                    }
                }
                """,
                "Shape" to shape, "body" to block, *SupportStructures.codegenScope,
            )
        }
    }
}

object SupportStructures {
    private val supportModule =
        RustModule.public("support", Module, documentationOverride = "Support traits and structures for serde")

    private val serde = CargoDependency.Serde.copy(scope = DependencyScope.Compile, optional = true).toType()

    val codegenScope =
        arrayOf(
            *preludeScope,
            "ConfigurableSerde" to configurableSerde(),
            "SerializeConfigured" to serializeConfigured(),
            "ConfigurableSerdeRef" to configurableSerdeRef(),
            "SerializationSettings" to serializationSettings(),
            "Sensitive" to sensitive(),
            "serde" to serde,
            "serialize_redacted" to serializeRedacted(),
            "serialize_unredacted" to serializeUnredacted(),
        )

    fun serializeRedacted(): RuntimeType =
        RuntimeType.forInlineFun("serialize_redacted", supportModule) {
            rustTemplate(
                """
                /// Serialize a value redacting sensitive fields
                pub fn serialize_redacted<'a, T, S: #{serde}::Serializer>(value: &'a T, serializer: S) -> Result<S::Ok, S::Error>
                where
                    T: #{SerializeConfigured},
                {
                    use #{serde}::Serialize;
                    value
                        .serialize_ref(&#{SerializationSettings} { redact_sensitive_fields: true })
                        .serialize(serializer)
                }
                """,
                "serde" to serde,
                "SerializeConfigured" to serializeConfigured(),
                "SerializationSettings" to serializationSettings(),
            )
        }

    fun serializeUnredacted(): RuntimeType =
        RuntimeType.forInlineFun("serialize_unredacted", supportModule) {
            rustTemplate(
                """
                /// Serialize a value without redacting sensitive fields
                pub fn serialize_unredacted<'a, T, S: #{serde}::Serializer>(value: &'a T, serializer: S) -> Result<S::Ok, S::Error>
                where
                    T: #{SerializeConfigured},
                {
                    use #{serde}::Serialize;
                    value
                        .serialize_ref(&#{SerializationSettings} { redact_sensitive_fields: false })
                        .serialize(serializer)
                }
                """,
                "serde" to serde,
                "SerializeConfigured" to serializeConfigured(),
                "SerializationSettings" to serializationSettings(),
            )
        }

    private fun serializeConfigured(): RuntimeType =
        RuntimeType.forInlineFun("SerializeConfigured", supportModule) {
            rustTemplate(
                """
                /// Trait that allows configuring serialization
                /// **This trait should not be implemented directly!** Instead, `impl Serialize for ConfigurableSerdeRef<T>`**
                pub trait SerializeConfigured {
                    /// Return a `Serialize` implementation for this object that owns the object. This is what you want
                    /// If you need to pass something that `impl`s serialize elsewhere.
                    fn serialize_owned(self, settings: #{SerializationSettings}) -> impl #{serde}::Serialize;

                    /// Return a `Serialize` implementation for this object that borrows from the given object
                    fn serialize_ref<'a>(&'a self, settings: &'a #{SerializationSettings}) -> impl #{serde}::Serialize + 'a;
                }

                /// Blanket implementation for all `T` that implement `ConfigurableSerdeRef
                impl<T> SerializeConfigured for T
                where
                    for<'a> #{ConfigurableSerdeRef}<'a, T>: #{serde}::Serialize,
                {
                    fn serialize_owned(
                        self,
                        settings: #{SerializationSettings},
                    ) -> impl #{serde}::Serialize {
                        #{ConfigurableSerde} {
                            value: self,
                            settings,
                        }
                    }

                    fn serialize_ref<'a>(
                        &'a self,
                        settings: &'a #{SerializationSettings},
                    ) -> impl #{serde}::Serialize + 'a {
                        #{ConfigurableSerdeRef} { value: self, settings }
                    }
                }
                """,
                "ConfigurableSerde" to configurableSerde(),
                "ConfigurableSerdeRef" to configurableSerdeRef(),
                "SerializationSettings" to serializationSettings(),
                "serde" to serde,
            )
        }

    private fun sensitive() =
        RuntimeType.forInlineFun("Sensitive", supportModule) {
            rustTemplate(
                """
                pub(crate) struct Sensitive<T>(pub(crate) T);

                impl<'a, T> ::serde::Serialize for ConfigurableSerdeRef<'a, Sensitive<T>>
                where
                T: #{serde}::Serialize,
                {
                    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
                    where
                    S: serde::Serializer,
                    {
                        match self.settings.redact_sensitive_fields {
                            true => serializer.serialize_str("<redacted>"),
                            false => self.value.0.serialize(serializer),
                        }
                    }
                }
                """,
                "serde" to CargoDependency.Serde.toType(),
            )
        }

    private fun configurableSerde() =
        RuntimeType.forInlineFun("ConfigurableSerde", supportModule) {
            rustTemplate(
                """
                ##[allow(missing_docs)]
                pub(crate) struct ConfigurableSerde<T> {
                    pub(crate) value: T,
                    pub(crate) settings: #{SerializationSettings}
                }

                impl<T> #{serde}::Serialize for ConfigurableSerde<T> where for <'a> ConfigurableSerdeRef<'a, T>: #{serde}::Serialize {
                    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
                    where
                        S: #{serde}::Serializer,
                    {
                        #{ConfigurableSerdeRef} {
                            value: &self.value,
                            settings: &self.settings,
                        }
                        .serialize(serializer)
                    }
                }

                """,
                "SerializationSettings" to serializationSettings(),
                "ConfigurableSerdeRef" to configurableSerdeRef(),
                "serde" to CargoDependency.Serde.toType(),
            )
        }

    private fun configurableSerdeRef() =
        RuntimeType.forInlineFun("ConfigurableSerdeRef", supportModule) {
            rustTemplate(
                """
                ##[allow(missing_docs)]
                pub(crate) struct ConfigurableSerdeRef<'a, T> {
                    pub(crate) value: &'a T,
                    pub(crate) settings: &'a SerializationSettings
                }
                """,
            )
        }

    private fun serializationSettings() =
        RuntimeType.forInlineFun("SerializationSettings", supportModule) {
            // TODO(serde): Add a builder for this structure and make it non-exhaustive
            // TODO(serde): Consider removing `derive(Default)`
            rustTemplate(
                """
                /// Settings for use when serializing structures
                ##[non_exhaustive]
                ##[derive(Copy, Clone, Debug, Default)]
                pub struct SerializationSettings {
                    /// Replace all sensitive fields with `<redacted>` during serialization
                    pub redact_sensitive_fields: bool,
                }

                impl SerializationSettings {
                    /// Replace all `@sensitive` fields with `<redacted>` when serializing.
                    ///
                    /// Note: This may alter the type of the serialized output and make it impossible to deserialize as
                    /// numerical fields will be replaced with strings.
                    pub fn redact_sensitive_fields() -> Self { Self { redact_sensitive_fields: true } }

                    /// Preserve the contents of sensitive fields during serializing
                    pub fn leak_sensitive_fields() -> Self { Self { redact_sensitive_fields: false } }
                }
                """,
            )
        }
}
