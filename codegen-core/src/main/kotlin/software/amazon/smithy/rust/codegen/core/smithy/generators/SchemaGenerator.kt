/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.generators

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
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.model.traits.Trait as SmithyTrait

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
                    "ShapeId" to smithySchema.resolve("ShapeId"),
                    "traits" to smithySchema.resolve("traits"),
                )
            for (trait in traits) {
                val typedInsert = typedTraitInsert(trait)
                if (typedInsert != null) {
                    rustTemplate(
                        """map.insert(Box::new(#{traits}::$typedInsert));""",
                        *codegenScope,
                    )
                } else {
                    val traitId = trait.toShapeId().toString().replace("#", "##")
                    val stringValue = trait.stringValue()
                    if (stringValue != null) {
                        rustTemplate(
                            """map.insert(Box::new(#{StringTrait}::new(#{ShapeId}::new("$traitId"), ${stringValue.dq()})));""",
                            *codegenScope,
                        )
                    } else {
                        rustTemplate(
                            """map.insert(Box::new(#{AnnotationTrait}::new(#{ShapeId}::new("$traitId"))));""",
                            *codegenScope,
                        )
                    }
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
