/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.generators

import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.shapes.BigDecimalShape
import software.amazon.smithy.model.shapes.BigIntegerShape
import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.shapes.BooleanShape
import software.amazon.smithy.model.shapes.ByteShape
import software.amazon.smithy.model.shapes.DocumentShape
import software.amazon.smithy.model.shapes.DoubleShape
import software.amazon.smithy.model.shapes.EnumShape
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
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.model.traits.SparseTrait
import software.amazon.smithy.model.traits.StreamingTrait
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.qualifiedName
import software.amazon.smithy.rust.codegen.core.rustlang.render
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.stripOuter
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.isOptional
import software.amazon.smithy.rust.codegen.core.smithy.isRustBoxed
import software.amazon.smithy.rust.codegen.core.smithy.rustType
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
    /** Extra field initializers for the deserialize constructor (e.g. `_request_id: None,`). */
    private val extraConstructFields: List<String> = emptyList(),
) {
    private val model = codegenContext.model
    private val symbolProvider = codegenContext.symbolProvider
    private val runtimeConfig = codegenContext.runtimeConfig
    private val smithySchema = RuntimeType.smithySchema(runtimeConfig)

    /** Sanitize a member name for use in Rust constant names (strips r# raw identifier prefix). */
    private fun constantName(memberName: String): String = memberName.removePrefix("r#").removePrefix("#").uppercase()

    /** Check if a shape is a string enum (EnumShape or StringShape with @enum trait). */
    private fun isStringEnum(shape: Shape): Boolean = shape is EnumShape || shape.hasTrait(EnumTrait::class.java)

    /**
     * Escape a member name for use inside rustTemplate strings.
     * Raw identifiers like `r#enum` contain `#`, which is the format character
     * in rustTemplate. We must escape `#` as `##` so that `r#enum` is emitted
     * as the literal Rust identifier rather than being parsed as a template
     * variable reference (`r` + `#{enum}`).
     */
    private fun templateEscape(name: String): String = name.replace("#", "##")

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

        val schemaPrefix = symbol.name.uppercase()
        val schemaStructName = "${symbol.name}Schema"

        // Write module-level statics and the schema unit struct
        val ns = shape.id.namespace
        val name = shape.id.name
        val fqn = shape.id.toString()
        val escapedFqn = fqn.replace("#", "##")
        writer.rustTemplate(
            """
            static ${schemaPrefix}_SCHEMA_ID: #{ShapeId} = #{ShapeId}::from_static("$escapedFqn", "$ns", "$name");
            static ${schemaPrefix}_SCHEMA_TRAITS: std::sync::LazyLock<#{TraitMap}> = std::sync::LazyLock::new(|| {
                // Allow unused mut: shapes with no serialization-relevant traits produce no insertions.
                ##[allow(unused_mut)]
                let mut map = #{TraitMap}::new();
                #{traitInsertions}
                map
            });
            """,
            *codegenScope,
            "traitInsertions" to generateTraitInsertions(shape),
        )
        renderMemberSchemas(writer, schemaPrefix)

        // Generate the single schema unit struct with the full Schema impl
        writer.rustTemplate(
            """
            /// Schema type for [`${symbol.name}`]. This zero-sized struct is the single
            /// source of truth for the shape's schema, used by both the `Schema` impl
            /// on the data type and by deserialization.
            pub struct $schemaStructName;
            """,
            *codegenScope,
        )
        writer.rustBlockTemplate("impl #{Schema} for $schemaStructName", *codegenScope) {
            rustTemplate(
                """
                fn shape_id(&self) -> &#{ShapeId} {
                    &${schemaPrefix}_SCHEMA_ID
                }

                fn shape_type(&self) -> #{ShapeType} {
                    #{ShapeType}::${shapeTypeVariant(shape)}
                }

                fn traits(&self) -> &#{TraitMap} {
                    &${schemaPrefix}_SCHEMA_TRAITS
                }
                """,
                *codegenScope,
            )
            renderMembers(writer, schemaPrefix)
        }
        writer.write("")

        // Provide access to the schema from the data type
        writer.rustTemplate(
            """
            impl ${symbol.name} {
                /// The schema for this shape.
                pub const SCHEMA: &'static $schemaStructName = &$schemaStructName;
            }
            """,
            *codegenScope,
        )

        // Write SerializableStruct impl for structures
        if (shape is StructureShape) {
            renderSerializableStruct(writer, symbol.name, schemaPrefix)
            renderDeserializeMethod(writer, symbol.name, schemaPrefix, schemaStructName)
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

        val memberWrites =
            writable {
                members.forEachIndexed { idx, member ->
                    val memberName = symbolProvider.toMemberName(member)
                    val memberSymbol = symbolProvider.toSymbol(member)
                    val target = model.expectShape(member.target)
                    val memberSchemaRef = "${schemaPrefix}_MEMBER_${constantName(memberName)}"
                    val writeCall = writeMethodForShape(target, memberSchemaRef)
                    if (memberSymbol.isOptional()) {
                        rust(
                            """
                            if let Some(ref val) = self.$memberName {
                                $writeCall
                            }
                            """,
                        )
                    } else {
                        rust(
                            """
                            {
                                let val = &self.$memberName;
                                $writeCall
                            }
                            """,
                        )
                    }
                }
            }

        writer.rustTemplate(
            """
            impl #{SerializableStruct} for $structName {
                fn serialize<S: #{ShapeSerializer}>(&self, serializer: &mut S) -> ::std::result::Result<(), S::Error> {
                    serializer.write_struct(Self::SCHEMA, |ser| {
                        self.serialize_members(ser)
                    })
                }
            }

            impl $structName {
                /// Writes this structure's members to the serializer without the struct wrapper.
                ##[allow(unused_variables, clippy::diverging_sub_expression)]
                pub fn serialize_members<S: #{ShapeSerializer}>(&self, ser: &mut S) -> ::std::result::Result<(), S::Error> {
                    #{memberWrites}
                    Ok(())
                }
            }
            """,
            *codegenScope,
            "memberWrites" to memberWrites,
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
            is BigIntegerShape -> "ser.write_big_integer(&$memberSchemaRef, val)?;"
            is BigDecimalShape -> "ser.write_big_decimal(&$memberSchemaRef, val)?;"
            is EnumShape -> "ser.write_string(&$memberSchemaRef, val.as_str())?;"
            is StringShape ->
                if (isStringEnum(target)) {
                    "ser.write_string(&$memberSchemaRef, val.as_str())?;"
                } else {
                    "ser.write_string(&$memberSchemaRef, val)?;"
                }
            is BlobShape ->
                if (target.hasTrait(StreamingTrait::class.java)) {
                    "// streaming blob is serialized as the HTTP body by the protocol, not the codec"
                } else {
                    "ser.write_blob(&$memberSchemaRef, val)?;"
                }
            is TimestampShape -> "ser.write_timestamp(&$memberSchemaRef, val)?;"
            is DocumentShape -> "ser.write_document(&$memberSchemaRef, val)?;"
            is ListShape -> {
                val isSparse = target.hasTrait(SparseTrait::class.java)
                val elementTarget = model.expectShape(target.member.target)
                val elementWrite = elementWriteExpr(elementTarget, "item")
                if (isSparse) {
                    """
                    ser.write_list(&$memberSchemaRef, |ser| {
                        for item in val {
                            match item {
                                Some(item) => { $elementWrite }
                                None => { ser.write_null(&aws_smithy_schema::prelude::STRING)?; }
                            }
                        }
                        Ok(())
                    })?;
                    """
                } else {
                    """
                    ser.write_list(&$memberSchemaRef, |ser| {
                        for item in val {
                            $elementWrite
                        }
                        Ok(())
                    })?;
                    """
                }
            }
            is MapShape -> {
                val isSparse = target.hasTrait(SparseTrait::class.java)
                val keyTarget = model.expectShape(target.key.target)
                val keyExpr = if (isStringEnum(keyTarget)) "key.as_str()" else "key"
                val valueTarget = model.expectShape(target.value.target)
                val valueShapeType = shapeTypeVariant(valueTarget)
                val valueWrite = mapValueWriteExpr(valueTarget, "value")
                if (isSparse) {
                    """
                    ser.write_map(&$memberSchemaRef, |ser| {
                        for (key, value) in val {
                            let entry = aws_smithy_schema::MapEntrySchema::new($keyExpr, aws_smithy_schema::ShapeType::$valueShapeType);
                            match value {
                                Some(value) => { $valueWrite }
                                None => { ser.write_null(&entry)?; }
                            }
                        }
                        Ok(())
                    })?;
                    """
                } else {
                    """
                    ser.write_map(&$memberSchemaRef, |ser| {
                        for (key, value) in val {
                            let entry = aws_smithy_schema::MapEntrySchema::new($keyExpr, aws_smithy_schema::ShapeType::$valueShapeType);
                            $valueWrite
                        }
                        Ok(())
                    })?;
                    """
                }
            }
            is StructureShape -> "ser.write_struct(&$memberSchemaRef, |ser| val.serialize_members(ser))?;"
            is UnionShape -> "ser.write_null(&$memberSchemaRef)?;"
            else -> "todo!(\"schema: unsupported shape type for serialization\");"
        }

    /** Returns a write expression for a list element (no member name needed). */
    private fun elementWriteExpr(
        target: Shape,
        varName: String,
    ): String {
        val prelude = "aws_smithy_schema::prelude"
        return when (target) {
            is BooleanShape -> "ser.write_boolean(&$prelude::BOOLEAN, *$varName)?;"
            is ByteShape -> "ser.write_byte(&$prelude::BYTE, *$varName)?;"
            is ShortShape -> "ser.write_short(&$prelude::SHORT, *$varName)?;"
            is IntegerShape -> "ser.write_integer(&$prelude::INTEGER, *$varName)?;"
            is LongShape -> "ser.write_long(&$prelude::LONG, *$varName)?;"
            is FloatShape -> "ser.write_float(&$prelude::FLOAT, *$varName)?;"
            is DoubleShape -> "ser.write_double(&$prelude::DOUBLE, *$varName)?;"
            is BigIntegerShape -> "ser.write_big_integer(&$prelude::BIG_INTEGER, $varName)?;"
            is BigDecimalShape -> "ser.write_big_decimal(&$prelude::BIG_DECIMAL, $varName)?;"
            is EnumShape -> "ser.write_string(&$prelude::STRING, $varName.as_str())?;"
            is StringShape ->
                if (isStringEnum(target)) {
                    "ser.write_string(&$prelude::STRING, $varName.as_str())?;"
                } else {
                    "ser.write_string(&$prelude::STRING, $varName)?;"
                }
            is BlobShape -> "ser.write_blob(&$prelude::BLOB, $varName)?;"
            is TimestampShape -> "ser.write_timestamp(&$prelude::TIMESTAMP, $varName)?;"
            is StructureShape -> {
                val targetQualified = symbolProvider.toSymbol(target).rustType().qualifiedName()
                "ser.write_struct($targetQualified::SCHEMA, |ser| $varName.serialize_members(ser))?;"
            }
            else -> "todo!(\"schema: unsupported list element type\");"
        }
    }

    /** Returns a write expression for a map value (uses `entry` schema for the key). */
    private fun mapValueWriteExpr(
        target: Shape,
        varName: String,
    ): String =
        when (target) {
            is BooleanShape -> "ser.write_boolean(&entry, *$varName)?;"
            is ByteShape -> "ser.write_byte(&entry, *$varName)?;"
            is ShortShape -> "ser.write_short(&entry, *$varName)?;"
            is IntegerShape -> "ser.write_integer(&entry, *$varName)?;"
            is LongShape -> "ser.write_long(&entry, *$varName)?;"
            is FloatShape -> "ser.write_float(&entry, *$varName)?;"
            is DoubleShape -> "ser.write_double(&entry, *$varName)?;"
            is BigIntegerShape -> "ser.write_big_integer(&entry, $varName)?;"
            is BigDecimalShape -> "ser.write_big_decimal(&entry, $varName)?;"
            is EnumShape -> "ser.write_string(&entry, $varName.as_str())?;"
            is StringShape ->
                if (isStringEnum(target)) {
                    "ser.write_string(&entry, $varName.as_str())?;"
                } else {
                    "ser.write_string(&entry, $varName)?;"
                }
            is BlobShape -> "ser.write_blob(&entry, $varName)?;"
            is TimestampShape -> "ser.write_timestamp(&entry, $varName)?;"
            is StructureShape -> "ser.write_struct(&entry, |ser| $varName.serialize_members(ser))?;"
            else -> "todo!(\"schema: unsupported map value type\");"
        }

    private fun renderDeserializeMethod(
        writer: RustWriter,
        structName: String,
        schemaPrefix: String,
        schemaStructName: String,
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
                pub fn deserialize<D: #{ShapeDeserializer}>(deserializer: &mut D) -> ::std::result::Result<Self, D::Error> {
                    ##[derive(Default)]
                    struct DeserializerBuilder {
                        #{builderFields}
                    }
                    let builder = DeserializerBuilder::default();
                    ##[allow(unused_variables, unused_mut, unreachable_code, clippy::single_match, clippy::match_single_binding, clippy::diverging_sub_expression)]
                    let builder = deserializer.read_struct(&$schemaStructName, builder, |mut builder, member, deser| {
                        match member.member_index() {
                            #{memberArms}
                            _ => {}
                        }
                        Ok(builder)
                    })?;
                    Ok($structName {
                        #{constructFields}
                    })
                }
            }
            """,
            *codegenScope,
            "builderFields" to
                writable {
                    members.forEach { member ->
                        val memberName = symbolProvider.toMemberName(member)
                        val memberSymbol = symbolProvider.toSymbol(member)
                        val target = model.expectShape(member.target)
                        val rustType = memberSymbol.rustType().stripOuter<software.amazon.smithy.rust.codegen.core.rustlang.RustType.Option>()
                        rust("$memberName: ::std::option::Option<${rustType.render()}>,")
                    }
                },
            "memberArms" to
                writable {
                    members.forEachIndexed { idx, member ->
                        val memberName = symbolProvider.toMemberName(member)
                        val memberSymbol = symbolProvider.toSymbol(member)
                        val target = model.expectShape(member.target)
                        val readExpr = readMethodForShape(target, "member")
                        val wrapped =
                            if (memberSymbol.isRustBoxed()) {
                                "Box::new($readExpr)"
                            } else {
                                readExpr
                            }
                        rust("Some($idx) => { builder.$memberName = Some($wrapped); }")
                    }
                },
            "constructFields" to
                writable {
                    members.forEach { member ->
                        val memberName = symbolProvider.toMemberName(member)
                        val memberSymbol = symbolProvider.toSymbol(member)
                        if (memberSymbol.isOptional()) {
                            rust("$memberName: builder.$memberName,")
                        } else {
                            val target = model.expectShape(member.target)
                            val fallback = defaultValueForShape(target)
                            if (fallback == "Default::default()") {
                                rust("$memberName: builder.$memberName.unwrap_or_default(),")
                            } else if (fallback != null) {
                                rust("$memberName: builder.$memberName.unwrap_or_else(|| $fallback),")
                            } else {
                                rust("$memberName: builder.$memberName.expect(${("missing required field: $memberName").dq()}),")
                            }
                        }
                    }
                    // Error shapes have an extra `meta` field added by ErrorGenerator
                    if (shape.hasTrait(ErrorTrait::class.java)) {
                        rust("meta: Default::default(),")
                    }
                    // Extra fields added by decorators (e.g. _request_id for output shapes)
                    for (field in extraConstructFields) {
                        rust(field)
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
            is BigIntegerShape -> "deser.read_big_integer($memberRef)?"
            is BigDecimalShape -> "deser.read_big_decimal($memberRef)?"
            is EnumShape -> {
                val enumName = symbolProvider.toSymbol(target).rustType().qualifiedName()
                "$enumName::from(deser.read_string($memberRef)?.as_str())"
            }
            is StringShape ->
                if (isStringEnum(target)) {
                    val enumName = symbolProvider.toSymbol(target).rustType().qualifiedName()
                    "$enumName::from(deser.read_string($memberRef)?.as_str())"
                } else {
                    "deser.read_string($memberRef)?"
                }
            is BlobShape ->
                if (target.hasTrait(StreamingTrait::class.java)) {
                    "{ let _ = $memberRef; ::aws_smithy_types::byte_stream::ByteStream::new(::aws_smithy_types::body::SdkBody::empty()) }"
                } else {
                    "deser.read_blob($memberRef)?"
                }
            is TimestampShape -> "deser.read_timestamp($memberRef)?"
            is DocumentShape -> "deser.read_document($memberRef)?"
            is ListShape -> {
                val isSparse = target.hasTrait(SparseTrait::class.java)
                val elementTarget = model.expectShape(target.member.target)
                val elementRead = elementReadExpr(elementTarget, memberRef)
                if (isSparse) {
                    "deser.read_list($memberRef, Vec::new(), |mut list, deser| { list.push(if deser.is_null() { deser.read_string($memberRef).ok(); None } else { Some($elementRead) }); Ok(list) })?"
                } else {
                    "deser.read_list($memberRef, Vec::new(), |mut list, deser| { list.push($elementRead); Ok(list) })?"
                }
            }
            is MapShape -> {
                val isSparse = target.hasTrait(SparseTrait::class.java)
                val keyTarget = model.expectShape(target.key.target)
                val keyInsert =
                    if (isStringEnum(keyTarget)) {
                        val enumName = symbolProvider.toSymbol(keyTarget).rustType().qualifiedName()
                        "$enumName::from(key.as_str())"
                    } else {
                        "key"
                    }
                val valueTarget = model.expectShape(target.value.target)
                val valueRead = elementReadExpr(valueTarget, memberRef)
                if (isSparse) {
                    "deser.read_map($memberRef, std::collections::HashMap::new(), |mut map, key, deser| { map.insert($keyInsert, if deser.is_null() { deser.read_string($memberRef).ok(); None } else { Some($valueRead) }); Ok(map) })?"
                } else {
                    "deser.read_map($memberRef, std::collections::HashMap::new(), |mut map, key, deser| { map.insert($keyInsert, $valueRead); Ok(map) })?"
                }
            }
            is StructureShape -> {
                val targetSymbol = symbolProvider.toSymbol(target)
                "${targetSymbol.rustType().qualifiedName()}::deserialize(deser)?"
            }
            else -> "{ let _ = $memberRef; todo!(\"deserialize aggregate\") }"
        }

    /** Returns a read expression for a list element or map value. */
    private fun elementReadExpr(
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
            is BigIntegerShape -> "deser.read_big_integer($memberRef)?"
            is BigDecimalShape -> "deser.read_big_decimal($memberRef)?"
            is EnumShape -> {
                val enumName = symbolProvider.toSymbol(target).rustType().qualifiedName()
                "$enumName::from(deser.read_string($memberRef)?.as_str())"
            }
            is StringShape ->
                if (isStringEnum(target)) {
                    val enumName = symbolProvider.toSymbol(target).rustType().qualifiedName()
                    "$enumName::from(deser.read_string($memberRef)?.as_str())"
                } else {
                    "deser.read_string($memberRef)?"
                }
            is BlobShape -> "deser.read_blob($memberRef)?"
            is TimestampShape -> "deser.read_timestamp($memberRef)?"
            is DocumentShape -> "deser.read_document($memberRef)?"
            is DocumentShape -> "deser.read_document($memberRef)?"
            is StructureShape -> {
                val targetSymbol = symbolProvider.toSymbol(target)
                "${targetSymbol.rustType().qualifiedName()}::deserialize(deser)?"
            }
            else -> "todo!(\"deserialize nested aggregate\")"
        }

    /** Returns a Rust default value expression for a shape, or null if no sensible default exists. */
    private fun defaultValueForShape(target: Shape): String? =
        when (target) {
            is BooleanShape -> "Default::default()"
            is ByteShape, is ShortShape, is IntegerShape, is LongShape -> "Default::default()"
            is FloatShape, is DoubleShape -> "Default::default()"
            is StringShape -> if (isStringEnum(target)) null else "Default::default()"
            is BlobShape ->
                if (target.hasTrait(StreamingTrait::class.java)) {
                    "::aws_smithy_types::byte_stream::ByteStream::new(::aws_smithy_types::body::SdkBody::empty())"
                } else {
                    "::aws_smithy_types::Blob::new(Vec::new())"
                }
            is TimestampShape -> "::aws_smithy_types::DateTime::from_secs(0)"
            is ListShape -> "Default::default()"
            is MapShape -> "Default::default()"
            else -> null
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
            is BigIntegerShape -> "BigInteger"
            is BigDecimalShape -> "BigDecimal"
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
                if (members.isEmpty()) {
                    // No members — default Schema trait methods already return None/empty
                } else {
                    val memberMatchArms =
                        members.joinToString("\n") { member ->
                            val memberName = symbolProvider.toMemberName(member)
                            val escapedName = templateEscape(memberName)
                            "\"$escapedName\" => Some(&${schemaPrefix}_MEMBER_${constantName(memberName)}),"
                        }
                    val indexMatchArms =
                        members.withIndex().joinToString("\n") { (idx, member) ->
                            val memberName = symbolProvider.toMemberName(member)
                            val escapedName = templateEscape(memberName)
                            "$idx => Some((\"$escapedName\", &${schemaPrefix}_MEMBER_${constantName(memberName)})),"
                        }
                    val membersArray =
                        members.joinToString(",\n") { member ->
                            val memberName = symbolProvider.toMemberName(member)
                            val escapedName = templateEscape(memberName)
                            "(\"$escapedName\", &${schemaPrefix}_MEMBER_${constantName(memberName)} as &dyn #{Schema})"
                        }
                    writer.rustTemplate(
                        """
                        fn member_schema(&self, name: &str) -> ::std::option::Option<&dyn #{Schema}> {
                            match name {
                                $memberMatchArms
                                _ => None,
                            }
                        }

                        fn member_schema_by_index(&self, index: usize) -> ::std::option::Option<(&str, &dyn #{Schema})> {
                            match index {
                                $indexMatchArms
                                _ => None,
                            }
                        }

                        fn members(&self) -> Box<dyn Iterator<Item = (&str, &dyn #{Schema})> + '_> {
                            Box::new([
                                $membersArray
                            ].into_iter())
                        }
                        """,
                        *codegenScope,
                    )
                }
            }

            is ListShape -> {
                writer.rustTemplate(
                    """
                    fn member(&self) -> ::std::option::Option<&dyn #{Schema}> {
                        Some(&${schemaPrefix}_MEMBER)
                    }
                    """,
                    *codegenScope,
                )
            }

            is MapShape -> {
                writer.rustTemplate(
                    """
                    fn key(&self) -> ::std::option::Option<&dyn #{Schema}> {
                        Some(&${schemaPrefix}_KEY)
                    }

                    fn member(&self) -> ::std::option::Option<&dyn #{Schema}> {
                        Some(&${schemaPrefix}_VALUE)
                    }
                    """,
                    *codegenScope,
                )
            }

            is MemberShape -> {
                writer.rust(
                    """
                    fn member_name(&self) -> ::std::option::Option<&str> {
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
                        static ${schemaPrefix}_MEMBER_${constantName(memberName)}: #{MemberSchema} = #{MemberSchema}::new(
                            #{ShapeId}::from_static(
                                "$escapedMemberId",
                                "${member.id.namespace}",
                                "${member.id.name}",
                            ),
                            #{ShapeType}::${shapeTypeVariant(target)},
                            ${templateEscape(memberName.dq())},
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
