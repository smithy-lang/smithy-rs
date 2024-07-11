/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.serde

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.neighbor.Walker
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.NumberShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.SensitiveTrait
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.DependencyScope
import software.amazon.smithy.rust.codegen.core.rustlang.Feature
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
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
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.isTargetUnit
import software.amazon.smithy.rust.codegen.core.util.letIf

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
        rustCrate.withModule(Module) {
            serializationRoots(codegenContext).forEach {
                this.addDependency(
                    serializerFn(
                        codegenContext,
                        it,
                    ).toSymbol(),
                )
            }
            addDependency(SupportStructures.serializeRedacted().toSymbol())
            addDependency(SupportStructures.serializeUnredacted().toSymbol())
        }
    }
}

fun serializerFn(
    codegenContext: CodegenContext,
    shape: Shape,
): RuntimeType {
    val name = codegenContext.symbolProvider.shapeFunctionName(codegenContext.serviceShape, shape) + "_serde"
    val symbolProvider = codegenContext.symbolProvider
    return RuntimeType.forInlineFun(name, Module) {
        rust("// serializers for $name")
        when (shape) {
            is StructureShape -> structSerdeImpl(codegenContext, shape)(this)
            is UnionShape -> serializeUnionImpl(codegenContext, shape)(this)
            // is MemberShape -> serializeMember(codegenContext, shape)(this)
            is StringShape, is NumberShape -> directSerde(codegenContext, shape)(this)
            else -> serializeWithTodo(codegenContext, shape)(this)
        }
    }
}

fun serializeWithTodo(
    codegenContext: CodegenContext,
    shape: Shape,
) = writable {
    val rustType = codegenContext.symbolProvider.toSymbol(shape)
    rustTemplate(
        """
        impl #{SerializeConfigured} for #{Shape} {}
        impl<'a> #{serde}::Serialize for #{ConfigurableSerdeRef}<'a, #{Shape}> {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where S: #{serde}::Serializer,
            {
                serializer.serialize_str("todo")
            }
        }
        """,
        *SupportStructures.codegenScope, "Shape" to rustType,
    )
}

fun directSerde(
    codegenContext: CodegenContext,
    shape: Shape,
): Writable {
    val rustType = codegenContext.symbolProvider.toSymbol(shape)
    val baseValue = writable { rust("self.value") }.letIf(shape.isStringShape) { it.plus(writable(".as_str()")) }
    return implSerializeConfigured(rustType) {
        rustTemplate("#{base}.serialize(serializer)", "base" to baseValue)
    }
}

fun serializeMember(
    codegenContext: CodegenContext,
    shape: MemberShape,
    memberRef: String,
): Writable {
    val target = codegenContext.model.expectShape(shape.target)
    return writable {
        val fieldName = codegenContext.symbolProvider.toMemberName(shape)
        addDependency(serializerFn(codegenContext, target).toSymbol())
        rust("&$memberRef.serialize_ref(&self.settings)")
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

fun structSerdeImpl(
    codegenContext: CodegenContext,
    shape: StructureShape,
): Writable {
    return implSerializeConfigured(codegenContext.symbolProvider.toSymbol(shape)) {
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
                        "member" to serializeMember(codegenContext, member, field),
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

fun serializeUnionImpl(
    codegenContext: CodegenContext,
    shape: UnionShape,
): Writable {
    val unionName = shape.contextName(codegenContext.serviceShape)
    val symbolProvider = codegenContext.symbolProvider
    val unionSymbol = symbolProvider.toSymbol(shape)

    return implSerializeConfigured(unionSymbol) {
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
                        "member" to serializeMember(codegenContext, member, "inner"),
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

private fun implSerializeConfigured(
    shape: Symbol,
    block: Writable,
): Writable {
    return writable {
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
                "serde" to serde, "SerializeConfigured" to serializeConfigured(), "SerializationSettings" to serializationSettings(),
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
                "serde" to serde, "SerializeConfigured" to serializeConfigured(), "SerializationSettings" to serializationSettings(),
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
                pub struct ConfigurableSerde<T> {
                    pub value: T,
                    pub settings: #{SerializationSettings}
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
                pub struct ConfigurableSerdeRef<'a, T> {
                    pub value: &'a T,
                    pub settings: &'a SerializationSettings
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
                ##[derive(Copy, Clone, Debug, Default)]
                pub struct SerializationSettings {
                    /// Replace all sensitive fields with `<redacted>` during serialization
                    pub redact_sensitive_fields: bool,
                }
                """,
            )
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
