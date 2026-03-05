/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.generators

import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.shapes.BooleanShape
import software.amazon.smithy.model.shapes.ByteShape
import software.amazon.smithy.model.shapes.DocumentShape
import software.amazon.smithy.model.shapes.DoubleShape
import software.amazon.smithy.model.shapes.FloatShape
import software.amazon.smithy.model.shapes.IntegerShape
import software.amazon.smithy.model.shapes.ListShape
import software.amazon.smithy.model.shapes.LongShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShortShape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.TimestampShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.isOptional
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.model.traits.Trait as SmithyTrait

/**
 * Allows custom rendering of a trait value in generated schema code.
 *
 * Implementations return a [Writable] that emits a Rust expression evaluating
 * to a `Box<dyn Trait>`, or null to use the default rendering.
 */
fun interface TraitCodegenProvider {
    fun render(trait: SmithyTrait): Writable?
}

/**
 * Registry of custom [TraitCodegenProvider]s keyed by trait Shape ID.
 *
 * Code generator extensions can register providers for custom traits so that
 * they are rendered with specific Rust types instead of the generic
 * [DocumentTrait] fallback.
 */
class SchemaTraitExtension {
    private val providers = mutableMapOf<software.amazon.smithy.model.shapes.ShapeId, TraitCodegenProvider>()

    fun add(
        traitId: software.amazon.smithy.model.shapes.ShapeId,
        provider: TraitCodegenProvider,
    ) {
        providers[traitId] = provider
    }

    fun providerFor(trait: SmithyTrait): TraitCodegenProvider? = providers[trait.toShapeId()]
}

/**
 * Generates Schema implementations for Smithy shapes.
 *
 * Schemas are runtime representations of shapes that enable protocol-agnostic
 * serialization and deserialization.
 */
class SchemaGenerator(
    private val codegenContext: CodegenContext,
    private val writer: RustWriter,
    private val shape: Shape,
    private val traitFilter: SchemaTraitFilter = SchemaTraitFilter(codegenContext.model),
    private val traitExtension: SchemaTraitExtension = SchemaTraitExtension(),
) {
    private val model = codegenContext.model
    private val symbolProvider = codegenContext.symbolProvider
    private val runtimeConfig = codegenContext.runtimeConfig
    private val smithySchema = RuntimeType.smithySchema(runtimeConfig)

    fun render() {
        val symbol = symbolProvider.toSymbol(shape)
        val codegenScope =
            arrayOf(
                "Schema" to smithySchema.resolve("Schema"),
                "ShapeId" to smithySchema.resolve("ShapeId"),
                "ShapeType" to smithySchema.resolve("ShapeType"),
                "TraitMap" to smithySchema.resolve("TraitMap"),
                "MemberSchema" to smithySchema.resolve("MemberSchema"),
            )

        // Write the Schema trait impl
        val schemaPrefix = symbol.name.uppercase()
        writer.rustBlockTemplate("impl #{Schema} for ${symbol.name}", *codegenScope) {
            rustTemplate(
                """
                fn shape_id(&self) -> &#{ShapeId} {
                    &${schemaPrefix}_SCHEMA_ID
                }

                fn shape_type(&self) -> #{ShapeType} {
                    #{ShapeType}::${shapeTypeVariant(shape)}
                }

                fn traits(&self) -> &#{TraitMap} {
                    static TRAITS: std::sync::LazyLock<#{TraitMap}> = std::sync::LazyLock::new(|| {
                        let mut map = #{TraitMap}::new();
                        #{traitInsertions}
                        map
                    });
                    &TRAITS
                }
                """,
                *codegenScope,
                "traitInsertions" to generateTraitInsertions(shape),
            )
            renderMembers(writer, schemaPrefix)
        }
        writer.write("")

        // Write module-level static schema constants
        val ns = shape.id.namespace
        val name = shape.id.name
        val fqn = shape.id.toString()
        val escapedFqn = fqn.replace("#", "##")
        writer.rustTemplate(
            """
            static ${schemaPrefix}_SCHEMA_ID: #{ShapeId} = #{ShapeId}::from_static("$escapedFqn", "$ns", "$name");
            """,
            *codegenScope,
        )
        renderMemberSchemas(writer, schemaPrefix)

        // Write SerializableStruct impl for structures
        if (shape is StructureShape) {
            renderSerializableStruct(writer, symbol.name, schemaPrefix)
            renderDeserializeMethod(writer, symbol.name, schemaPrefix)
        }
    }

    private fun renderSerializableStruct(
        writer: RustWriter,
        structName: String,
        schemaPrefix: String,
    ) {
        val codegenScope =
            arrayOf(
                "SerializableStruct" to smithySchema.resolve("serde::SerializableStruct"),
                "ShapeSerializer" to smithySchema.resolve("serde::ShapeSerializer"),
            )
        val members = (shape as StructureShape).allMembers.values.toList()

        writer.rustTemplate(
            """
            impl #{SerializableStruct} for $structName {
                fn serialize<S: #{ShapeSerializer}>(&self, serializer: &mut S) -> Result<(), S::Error> {
                    serializer.write_struct(self, |ser| {
                        #{memberWrites}
                        Ok(())
                    })
                }
            }
            """,
            *codegenScope,
            "memberWrites" to
                writable {
                    members.forEachIndexed { idx, member ->
                        val memberName = symbolProvider.toMemberName(member)
                        val memberSymbol = symbolProvider.toSymbol(member)
                        val target = model.expectShape(member.target)
                        val writeCall = writeMethodForShape(target, "${schemaPrefix}_MEMBER_${memberName.uppercase()}")
                        if (memberSymbol.isOptional()) {
                            rust(
                                """
                                if let Some(ref val) = self.$memberName {
                                    $writeCall
                                }
                                """,
                            )
                        } else {
                            rust(writeCall)
                        }
                    }
                },
        )
    }

    /**
     * Returns a Rust expression that writes a value to a serializer.
     * For optional fields, `val` is the unwrapped reference.
     * For non-optional fields, `self.field_name` is used directly.
     */
    private fun writeMethodForShape(
        target: Shape,
        memberSchemaRef: String,
    ): String =
        when (target) {
            is BooleanShape -> "ser.write_boolean(&$memberSchemaRef, *val)?;"
            is ByteShape -> "ser.write_byte(&$memberSchemaRef, *val)?;"
            is ShortShape -> "ser.write_short(&$memberSchemaRef, *val)?;"
            is IntegerShape -> "ser.write_integer(&$memberSchemaRef, *val)?;"
            is LongShape -> "ser.write_long(&$memberSchemaRef, *val)?;"
            is FloatShape -> "ser.write_float(&$memberSchemaRef, *val)?;"
            is DoubleShape -> "ser.write_double(&$memberSchemaRef, *val)?;"
            is StringShape -> "ser.write_string(&$memberSchemaRef, val)?;"
            is BlobShape -> "ser.write_blob(&$memberSchemaRef, val)?;"
            is TimestampShape -> "ser.write_timestamp(&$memberSchemaRef, val)?;"
            is DocumentShape -> "ser.write_document(&$memberSchemaRef, val)?;"
            is ListShape -> "ser.write_list(&$memberSchemaRef, |_ser| { Ok(()) })?;"
            is MapShape -> "ser.write_map(&$memberSchemaRef, |_ser| { Ok(()) })?;"
            is StructureShape -> "val.serialize(ser)?;"
            is UnionShape -> "ser.write_null(&$memberSchemaRef)?;"
            else -> "// TODO(schema) unsupported shape type for serialization"
        }

    private fun renderDeserializeMethod(
        writer: RustWriter,
        structName: String,
        schemaPrefix: String,
    ) {
        val codegenScope =
            arrayOf(
                "ShapeDeserializer" to smithySchema.resolve("serde::ShapeDeserializer"),
                "Schema" to smithySchema.resolve("Schema"),
            )
        val members = (shape as StructureShape).allMembers.values.toList()

        writer.rustTemplate(
            """
            impl $structName {
                /// Deserializes this structure from a [`ShapeDeserializer`].
                pub fn deserialize<D: #{ShapeDeserializer}>(deserializer: &mut D) -> Result<Self, D::Error> {
                    let schema = $structName {
                        #{defaultFields}
                    };
                    let builder = $structName {
                        #{defaultFields}
                    };
                    deserializer.read_struct(&schema, builder, |mut builder, member, deser| {
                        match member.member_index() {
                            #{memberArms}
                            _ => {}
                        }
                        Ok(builder)
                    })
                }
            }
            """,
            *codegenScope,
            "defaultFields" to
                writable {
                    members.forEach { member ->
                        val memberName = symbolProvider.toMemberName(member)
                        val memberSymbol = symbolProvider.toSymbol(member)
                        if (memberSymbol.isOptional()) {
                            rust("$memberName: None,")
                        } else {
                            rust("$memberName: Default::default(),")
                        }
                    }
                },
            "memberArms" to
                writable {
                    members.forEachIndexed { idx, member ->
                        val memberName = symbolProvider.toMemberName(member)
                        val target = model.expectShape(member.target)
                        val readExpr = readMethodForShape(target, "member")
                        rust("Some($idx) => { builder.$memberName = Some($readExpr); }")
                    }
                },
        )
    }

    private fun readMethodForShape(
        target: Shape,
        memberRef: String,
    ): String =
        when (target) {
            is BooleanShape -> "deser.read_boolean($memberRef)?"
            is ByteShape -> "deser.read_byte($memberRef)?"
            is ShortShape -> "deser.read_short($memberRef)?"
            is IntegerShape -> "deser.read_integer($memberRef)?"
            is LongShape -> "deser.read_long($memberRef)?"
            is FloatShape -> "deser.read_float($memberRef)?"
            is DoubleShape -> "deser.read_double($memberRef)?"
            is StringShape -> "deser.read_string($memberRef)?"
            is BlobShape -> "deser.read_blob($memberRef)?"
            is TimestampShape -> "deser.read_timestamp($memberRef)?"
            is DocumentShape -> "deser.read_document($memberRef)?"
            else -> "{ let _ = $memberRef; todo!(\"deserialize aggregate\") }"
        }

    private fun shapeTypeVariant(shape: Shape): String =
        when (shape) {
            is BooleanShape -> "Boolean"
            is ByteShape -> "Byte"
            is ShortShape -> "Short"
            is IntegerShape -> "Integer"
            is LongShape -> "Long"
            is FloatShape -> "Float"
            is DoubleShape -> "Double"
            is StringShape -> "String"
            is BlobShape -> "Blob"
            is TimestampShape -> "Timestamp"
            is DocumentShape -> "Document"
            is ListShape -> "List"
            is MapShape -> "Map"
            is StructureShape -> "Structure"
            is UnionShape -> "Union"
            is MemberShape -> "Member"
            else -> throw IllegalArgumentException("Unsupported shape type: ${shape.type}")
        }

    /**
     * Generates a Writable that emits `map.insert(...)` calls for each
     * included trait on the given shape.
     */
    private fun generateTraitInsertions(shape: Shape) =
        writable {
            val traits = traitFilter.traitsFor(shape)
            val codegenScope =
                arrayOf(
                    "AnnotationTrait" to smithySchema.resolve("AnnotationTrait"),
                    "StringTrait" to smithySchema.resolve("StringTrait"),
                    "DocumentTrait" to smithySchema.resolve("DocumentTrait"),
                    "ShapeId" to smithySchema.resolve("ShapeId"),
                    "Document" to RuntimeType.smithyTypes(runtimeConfig).resolve("Document"),
                    "traits" to smithySchema.resolve("traits"),
                )
            for (trait in traits) {
                // 1. Check extension for custom rendering
                val customProvider = traitExtension.providerFor(trait)
                if (customProvider != null) {
                    val customWritable = customProvider.render(trait)
                    if (customWritable != null) {
                        rust("map.insert(")
                        customWritable(this)
                        rust(");")
                        continue
                    }
                }

                // 2. Use typed trait if available
                val typedInsert = typedTraitInsert(trait)
                if (typedInsert != null) {
                    rustTemplate(
                        """map.insert(Box::new(#{traits}::$typedInsert));""",
                        *codegenScope,
                    )
                    continue
                }

                // 3. Fall back: annotation, string, or document
                val traitId = trait.toShapeId().toString().replace("#", "##")
                val stringValue = trait.stringValue()
                if (trait.isAnnotationTrait()) {
                    rustTemplate(
                        """map.insert(Box::new(#{AnnotationTrait}::new(#{ShapeId}::new("$traitId"))));""",
                        *codegenScope,
                    )
                } else if (stringValue != null) {
                    rustTemplate(
                        """map.insert(Box::new(#{StringTrait}::new(#{ShapeId}::new("$traitId"), ${stringValue.dq()})));""",
                        *codegenScope,
                    )
                } else {
                    // TODO(schema) Evaluate creating a Document that fully models the trait data
                    //  rather than holding it as a JSON string. The existing serde trait
                    //  implementations on aws_smithy_types::Document could be used to convert
                    //  the Smithy Node directly into a structured Document value.
                    val jsonValue = Node.printJson(trait.toNode()).replace("\\", "\\\\").replace("\"", "\\\"")
                    rustTemplate(
                        """map.insert(Box::new(#{DocumentTrait}::new(#{ShapeId}::new("$traitId"), #{Document}::String("$jsonValue".to_string()))));""",
                        *codegenScope,
                    )
                }
            }
        }

    /**
     * Returns a Rust expression for constructing a typed trait, or null if no
     * typed representation exists (falls back to generic AnnotationTrait/StringTrait).
     */
    private fun typedTraitInsert(trait: SmithyTrait): String? {
        val id = trait.toShapeId().toString()
        val stringValue = trait.stringValue()
        return when (id) {
            // String-valued traits
            "smithy.api#jsonName" -> "JsonNameTrait::new(${stringValue!!.dq()})"
            "smithy.api#xmlName" -> "XmlNameTrait::new(${stringValue!!.dq()})"
            "smithy.api#mediaType" -> "MediaTypeTrait::new(${stringValue!!.dq()})"
            "smithy.api#timestampFormat" -> "TimestampFormatTrait::new(${stringValue!!.dq()})"
            "smithy.api#httpHeader" -> "HttpHeaderTrait::new(${stringValue!!.dq()})"
            "smithy.api#httpQuery" -> "HttpQueryTrait::new(${stringValue!!.dq()})"
            "smithy.api#httpPrefixHeaders" -> "HttpPrefixHeadersTrait::new(${stringValue!!.dq()})"
            // Annotation traits
            "smithy.api#sensitive" -> "SensitiveTrait"
            "smithy.api#xmlAttribute" -> "XmlAttributeTrait"
            "smithy.api#xmlFlattened" -> "XmlFlattenedTrait"
            "smithy.api#httpLabel" -> "HttpLabelTrait"
            "smithy.api#httpPayload" -> "HttpPayloadTrait"
            "smithy.api#httpQueryParams" -> "HttpQueryParamsTrait"
            "smithy.api#httpResponseCode" -> "HttpResponseCodeTrait"
            "smithy.api#streaming" -> "StreamingTrait"
            "smithy.api#eventHeader" -> "EventHeaderTrait"
            "smithy.api#eventPayload" -> "EventPayloadTrait"
            "smithy.api#hostLabel" -> "HostLabelTrait"
            else -> null
        }
    }

    private fun renderMembers(
        writer: RustWriter,
        schemaPrefix: String,
    ) {
        val codegenScope =
            arrayOf(
                "Schema" to smithySchema.resolve("Schema"),
            )

        when (shape) {
            is StructureShape, is UnionShape -> {
                val members = shape.members()
                writer.rustTemplate(
                    """
                    fn member_schema(&self, name: &str) -> Option<&dyn #{Schema}> {
                        match name {
                            ${
                        members.joinToString("\n                            ") { member ->
                            val memberName = symbolProvider.toMemberName(member)
                            "\"$memberName\" => Some(&${schemaPrefix}_MEMBER_${memberName.uppercase()}),"
                        }
                    }
                            _ => None,
                        }
                    }

                    fn member_schema_by_index(&self, index: usize) -> Option<(&str, &dyn #{Schema})> {
                        match index {
                            ${
                        members.withIndex().joinToString("\n                            ") { (idx, member) ->
                            val memberName = symbolProvider.toMemberName(member)
                            "$idx => Some((\"$memberName\", &${schemaPrefix}_MEMBER_${memberName.uppercase()})),"
                        }
                    }
                            _ => None,
                        }
                    }

                    fn members(&self) -> Box<dyn Iterator<Item = (&str, &dyn #{Schema})> + '_> {
                        Box::new([
                            ${
                        members.joinToString(",\n                            ") { member ->
                            val memberName = symbolProvider.toMemberName(member)
                            "(\"$memberName\", &${schemaPrefix}_MEMBER_${memberName.uppercase()} as &dyn #{Schema})"
                        }
                    }
                        ].into_iter())
                    }
                    """,
                    *codegenScope,
                )
            }

            is ListShape -> {
                writer.rustTemplate(
                    """
                    fn member(&self) -> Option<&dyn #{Schema}> {
                        Some(&${schemaPrefix}_MEMBER)
                    }
                    """,
                    *codegenScope,
                )
            }

            is MapShape -> {
                writer.rustTemplate(
                    """
                    fn key(&self) -> Option<&dyn #{Schema}> {
                        Some(&${schemaPrefix}_KEY)
                    }

                    fn member(&self) -> Option<&dyn #{Schema}> {
                        Some(&${schemaPrefix}_VALUE)
                    }
                    """,
                    *codegenScope,
                )
            }

            is MemberShape -> {
                writer.rust(
                    """
                    fn member_name(&self) -> Option<&str> {
                        Some(${symbolProvider.toMemberName(shape).dq()})
                    }
                    """,
                )
            }
        }
    }

    private fun renderMemberSchemas(
        writer: RustWriter,
        schemaPrefix: String,
    ) {
        val codegenScope =
            arrayOf(
                "MemberSchema" to smithySchema.resolve("MemberSchema"),
                "ShapeId" to smithySchema.resolve("ShapeId"),
                "ShapeType" to smithySchema.resolve("ShapeType"),
            )

        when (shape) {
            is StructureShape, is UnionShape -> {
                shape.members().forEachIndexed { idx, member ->
                    val memberName = symbolProvider.toMemberName(member)
                    val target = model.expectShape(member.target)
                    val escapedMemberId = member.id.toString().replace("#", "##")
                    writer.rustTemplate(
                        """
                        static ${schemaPrefix}_MEMBER_${memberName.uppercase()}: #{MemberSchema} = #{MemberSchema}::new(
                            #{ShapeId}::from_static(
                                "$escapedMemberId",
                                "${member.id.namespace}",
                                "${member.id.name}",
                            ),
                            #{ShapeType}::${shapeTypeVariant(target)},
                            ${memberName.dq()},
                            $idx,
                        );
                        """,
                        *codegenScope,
                    )
                }
            }

            is ListShape -> {
                val target = model.expectShape(shape.member.target)
                val escapedMemberId = shape.member.id.toString().replace("#", "##")
                writer.rustTemplate(
                    """
                    static ${schemaPrefix}_MEMBER: #{MemberSchema} = #{MemberSchema}::new(
                        #{ShapeId}::from_static(
                            "$escapedMemberId",
                            "${shape.member.id.namespace}",
                            "${shape.member.id.name}",
                        ),
                        #{ShapeType}::${shapeTypeVariant(target)},
                        "member",
                        0,
                    );
                    """,
                    *codegenScope,
                )
            }

            is MapShape -> {
                val keyTarget = model.expectShape(shape.key.target)
                val valueTarget = model.expectShape(shape.value.target)
                val escapedKeyId = shape.key.id.toString().replace("#", "##")
                val escapedValueId = shape.value.id.toString().replace("#", "##")
                writer.rustTemplate(
                    """
                    static ${schemaPrefix}_KEY: #{MemberSchema} = #{MemberSchema}::new(
                        #{ShapeId}::from_static(
                            "$escapedKeyId",
                            "${shape.key.id.namespace}",
                            "${shape.key.id.name}",
                        ),
                        #{ShapeType}::${shapeTypeVariant(keyTarget)},
                        "key",
                        0,
                    );

                    static ${schemaPrefix}_VALUE: #{MemberSchema} = #{MemberSchema}::new(
                        #{ShapeId}::from_static(
                            "$escapedValueId",
                            "${shape.value.id.namespace}",
                            "${shape.value.id.name}",
                        ),
                        #{ShapeType}::${shapeTypeVariant(valueTarget)},
                        "value",
                        1,
                    );
                    """,
                    *codegenScope,
                )
            }
        }
    }
}
