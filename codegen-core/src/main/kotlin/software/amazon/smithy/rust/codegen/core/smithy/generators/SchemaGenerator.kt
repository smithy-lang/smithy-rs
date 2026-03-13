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
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.qualifiedName
import software.amazon.smithy.rust.codegen.core.rustlang.render
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
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
            )

        val schemaPrefix = symbol.name.uppercase()

        // Write module-level statics and the schema unit struct
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

        // Generate the static Schema value
        renderSchemaStatic(writer, schemaPrefix, symbol.name)

        // Provide access to the schema from the data type
        writer.rustTemplate(
            """
            impl ${symbol.name} {
                /// The schema for this shape.
                pub const SCHEMA: &'static #{Schema} = &${schemaPrefix}_SCHEMA;
            }
            """,
            *codegenScope,
        )

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
                "SerdeError" to smithySchema.resolve("serde::SerdeError"),
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
                ##[allow(unused_variables, clippy::diverging_sub_expression)]
                fn serialize_members(&self, ser: &mut dyn #{ShapeSerializer}) -> ::std::result::Result<(), #{SerdeError}> {
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
                    ser.write_list(&$memberSchemaRef, &|ser: &mut dyn ::aws_smithy_schema::serde::ShapeSerializer| {
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
                    ser.write_list(&$memberSchemaRef, &|ser: &mut dyn ::aws_smithy_schema::serde::ShapeSerializer| {
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
                val valueWrite = mapValueWriteExpr(valueTarget, "value")
                if (isSparse) {
                    """
                    ser.write_map(&$memberSchemaRef, &|ser: &mut dyn ::aws_smithy_schema::serde::ShapeSerializer| {
                        for (key, value) in val {
                            ser.write_string(&::aws_smithy_schema::prelude::STRING, $keyExpr)?;
                            match value {
                                Some(value) => { $valueWrite }
                                None => { ser.write_null(&::aws_smithy_schema::prelude::STRING)?; }
                            }
                        }
                        Ok(())
                    })?;
                    """
                } else {
                    """
                    ser.write_map(&$memberSchemaRef, &|ser: &mut dyn ::aws_smithy_schema::serde::ShapeSerializer| {
                        for (key, value) in val {
                            ser.write_string(&::aws_smithy_schema::prelude::STRING, $keyExpr)?;
                            $valueWrite
                        }
                        Ok(())
                    })?;
                    """
                }
            }

            is StructureShape -> "ser.write_struct(&$memberSchemaRef, val)?;"
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
                "ser.write_struct($targetQualified::SCHEMA, $varName)?;"
            }

            else -> "todo!(\"schema: unsupported list element type\");"
        }
    }

    /** Returns a write expression for a map value. */
    private fun mapValueWriteExpr(
        target: Shape,
        varName: String,
    ): String {
        val prelude = "::aws_smithy_schema::prelude"
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
                "ser.write_struct($targetQualified::SCHEMA, $varName)?;"
            }

            else -> "todo!(\"schema: unsupported map value type\");"
        }
    }

    private fun renderDeserializeMethod(
        writer: RustWriter,
        structName: String,
        schemaPrefix: String,
    ) {
        val codegenScope =
            arrayOf(
                "ShapeDeserializer" to smithySchema.resolve("serde::ShapeDeserializer"),
                "SerdeError" to smithySchema.resolve("serde::SerdeError"),
                "Schema" to smithySchema.resolve("Schema"),
                "Builder" to symbolProvider.symbolForBuilder(shape),
            )
        val members = (shape as StructureShape).allMembers.values.toList()

        writer.rustTemplate(
            """
            impl $structName {
                /// Deserializes this structure from a [`ShapeDeserializer`].
                pub fn deserialize<D: #{ShapeDeserializer}>(deserializer: &mut D) -> ::std::result::Result<Self, #{SerdeError}> {
                    ##[allow(unused_variables, unused_mut)]
                    let mut builder = #{Builder}::default();
                    ##[allow(unused_variables, unreachable_code, clippy::single_match, clippy::match_single_binding, clippy::diverging_sub_expression)]
                    deserializer.read_struct(&${schemaPrefix}_SCHEMA, (), |_, member, deser| {
                        match member.member_index() {
                            #{memberArms}
                            _ => {}
                        }
                        Ok(())
                    })?;
                    Ok($structName {
                        #{constructFields}
                    })
                }
            }
            """,
            *codegenScope,
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
                val pushExpr =
                    if (isSparse) {
                        "list.push(if deser.is_null() { deser.read_string($memberRef).ok(); None } else { Some($elementRead) })"
                    } else {
                        "list.push($elementRead)"
                    }
                "{ let container = if let Some(cap) = deser.container_size() { Vec::with_capacity(cap) } else { Vec::new() }; deser.read_list($memberRef, container, |mut list, deser| { $pushExpr; Ok(list) })? }"
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
                val insertExpr =
                    if (isSparse) {
                        "map.insert($keyInsert, if deser.is_null() { deser.read_string($memberRef).ok(); None } else { Some($valueRead) })"
                    } else {
                        "map.insert($keyInsert, $valueRead)"
                    }
                "{ let container = if let Some(cap) = deser.container_size() { std::collections::HashMap::with_capacity(cap) } else { std::collections::HashMap::new() }; deser.read_map($memberRef, container, |mut map, key, deser| { $insertExpr; Ok(map) })? }"
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

    /** Generates `map.insert(...)` calls for traits that are NOT known direct fields on Schema. */
    private fun generateUnknownTraitInsertions(shape: Shape) =
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
                // Skip known traits — they're handled by with_*() setters
                if (knownTraitSetter(trait) != null) continue

                // Check extension for custom rendering
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

                // Fall back: annotation, string, or document
                val traitNs = trait.toShapeId().namespace
                val traitName = trait.toShapeId().name
                val stringValue = trait.stringValue()
                if (trait.isAnnotationTrait()) {
                    rustTemplate(
                        """map.insert(Box::new(#{AnnotationTrait}::new(#{ShapeId}::from_static("$traitNs##$traitName", "$traitNs", "$traitName"))));""",
                        *codegenScope,
                    )
                } else if (stringValue != null) {
                    rustTemplate(
                        """map.insert(Box::new(#{StringTrait}::new(#{ShapeId}::from_static("$traitNs##$traitName", "$traitNs", "$traitName"), ${stringValue.dq()})));""",
                        *codegenScope,
                    )
                } else {
                    val jsonValue = Node.printJson(trait.toNode()).replace("\\", "\\\\").replace("\"", "\\\"")
                    rustTemplate(
                        """map.insert(Box::new(#{DocumentTrait}::new(#{ShapeId}::from_static("$traitNs##$traitName", "$traitNs", "$traitName"), #{Document}::String("$jsonValue".to_string()))));""",
                        *codegenScope,
                    )
                }
            }
        }

    /**
     * Returns the `.with_*()` chain for known serde traits on a shape.
     * Returns empty string if the shape has no known traits.
     */
    private fun traitSetterChain(shape: Shape): String {
        val setters = mutableListOf<String>()
        for (trait in traitFilter.traitsFor(shape)) {
            val setter = knownTraitSetter(trait)
            if (setter != null) {
                setters.add(setter)
            }
        }
        return setters.joinToString("")
    }

    /** Returns true if the shape has any filtered traits that are NOT known direct fields. */
    private fun hasUnknownTraits(shape: Shape): Boolean =
        traitFilter.traitsFor(shape).any { knownTraitSetter(it) == null }

    /**
     * Returns a `.with_*()` call for a known trait, or null if the trait
     * is not a known direct field on Schema.
     *
     * IMPORTANT: This must stay in sync with the `with_*` setters and known trait
     * fields on `Schema` in `aws-smithy-schema/src/lib.rs`. If a new known trait
     * is added to `Schema`, a corresponding entry must be added here.
     */
    private fun knownTraitSetter(trait: software.amazon.smithy.model.traits.Trait): String? {
        val id = trait.toShapeId().toString()
        val stringValue = trait.stringValue()
        return when (id) {
            "smithy.api#sensitive" -> "\n    .with_sensitive()"
            "smithy.api#jsonName" -> "\n    .with_json_name(${stringValue!!.dq()})"
            "smithy.api#timestampFormat" -> {
                val variant =
                    when (stringValue) {
                        "epoch-seconds" -> "EpochSeconds"
                        "date-time" -> "DateTime"
                        "http-date" -> "HttpDate"
                        else -> return null
                    }
                "\n    .with_timestamp_format(aws_smithy_schema::traits::TimestampFormat::$variant)"
            }

            "smithy.api#xmlName" -> "\n    .with_xml_name(${stringValue!!.dq()})"
            "smithy.api#xmlAttribute" -> "\n    .with_xml_attribute()"
            "smithy.api#xmlFlattened" -> "\n    .with_xml_flattened()"
            "smithy.api#xmlNamespace" -> "\n    .with_xml_namespace()"
            "smithy.api#mediaType" -> "\n    .with_media_type(${stringValue!!.dq()})"
            "smithy.api#httpHeader" -> "\n    .with_http_header(${stringValue!!.dq()})"
            "smithy.api#httpLabel" -> "\n    .with_http_label()"
            "smithy.api#httpPayload" -> "\n    .with_http_payload()"
            "smithy.api#httpPrefixHeaders" -> "\n    .with_http_prefix_headers(${stringValue!!.dq()})"
            "smithy.api#httpQuery" -> "\n    .with_http_query(${stringValue!!.dq()})"
            "smithy.api#httpQueryParams" -> "\n    .with_http_query_params()"
            "smithy.api#httpResponseCode" -> "\n    .with_http_response_code()"
            "smithy.api#streaming" -> "\n    .with_streaming()"
            "smithy.api#eventHeader" -> "\n    .with_event_header()"
            "smithy.api#eventPayload" -> "\n    .with_event_payload()"
            "smithy.api#hostLabel" -> "\n    .with_host_label()"
            else -> null
        }
    }

    private fun renderSchemaStatic(
        writer: RustWriter,
        schemaPrefix: String,
        structName: String,
    ) {
        val codegenScope =
            arrayOf(
                "Schema" to smithySchema.resolve("Schema"),
                "ShapeType" to smithySchema.resolve("ShapeType"),
            )

        when (shape) {
            is StructureShape, is UnionShape -> {
                val members = shape.members()
                val membersArray =
                    if (members.isEmpty()) {
                        "&[]"
                    } else {
                        val refs =
                            members.joinToString(", ") { member ->
                                val memberName = symbolProvider.toMemberName(member)
                                "&${schemaPrefix}_MEMBER_${constantName(memberName)}"
                            }
                        "&[$refs]"
                    }
                val traitChain = traitSetterChain(shape)
                if (hasUnknownTraits(shape)) {
                    writer.rustTemplate(
                        """
                        static ${schemaPrefix}_TRAITS: std::sync::LazyLock<#{TraitMap}> = std::sync::LazyLock::new(|| {
                            let mut map = #{TraitMap}::new();
                            #{insertions}
                            map
                        });
                        static ${schemaPrefix}_SCHEMA: #{Schema} = #{Schema}::new_struct(
                            ${schemaPrefix}_SCHEMA_ID,
                            #{ShapeType}::${shapeTypeVariant(shape)},
                            $membersArray,
                        )$traitChain
                        .with_traits(&${schemaPrefix}_TRAITS);
                        """,
                        *codegenScope,
                        "TraitMap" to smithySchema.resolve("TraitMap"),
                        "insertions" to generateUnknownTraitInsertions(shape),
                    )
                } else {
                    writer.rustTemplate(
                        """
                        static ${schemaPrefix}_SCHEMA: #{Schema} = #{Schema}::new_struct(
                            ${schemaPrefix}_SCHEMA_ID,
                            #{ShapeType}::${shapeTypeVariant(shape)},
                            $membersArray,
                        )$traitChain;
                        """,
                        *codegenScope,
                    )
                }
            }

            is ListShape -> {
                writer.rustTemplate(
                    """
                    static ${schemaPrefix}_SCHEMA: #{Schema} = #{Schema}::new_list(
                        ${schemaPrefix}_SCHEMA_ID,
                        &${schemaPrefix}_MEMBER,
                    );
                    """,
                    *codegenScope,
                )
            }

            is MapShape -> {
                writer.rustTemplate(
                    """
                    static ${schemaPrefix}_SCHEMA: #{Schema} = #{Schema}::new_map(
                        ${schemaPrefix}_SCHEMA_ID,
                        &${schemaPrefix}_KEY,
                        &${schemaPrefix}_VALUE,
                    );
                    """,
                    *codegenScope,
                )
            }

            else -> {
                writer.rustTemplate(
                    """
                    static ${schemaPrefix}_SCHEMA: #{Schema} = #{Schema}::new(
                        ${schemaPrefix}_SCHEMA_ID,
                        #{ShapeType}::${shapeTypeVariant(shape)},
                    );
                    """,
                    *codegenScope,
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
                "Schema" to smithySchema.resolve("Schema"),
                "ShapeId" to smithySchema.resolve("ShapeId"),
                "ShapeType" to smithySchema.resolve("ShapeType"),
            )

        when (shape) {
            is StructureShape, is UnionShape -> {
                shape.members().forEachIndexed { idx, member ->
                    val memberName = symbolProvider.toMemberName(member)
                    val target = model.expectShape(member.target)
                    val escapedMemberId = member.id.toString().replace("#", "##")
                    val memberTraitChain = traitSetterChain(member)
                    writer.rustTemplate(
                        """
                        static ${schemaPrefix}_MEMBER_${constantName(memberName)}: #{Schema} = #{Schema}::new_member(
                            #{ShapeId}::from_static(
                                "$escapedMemberId",
                                "${member.id.namespace}",
                                "${member.id.name}",
                            ),
                            #{ShapeType}::${shapeTypeVariant(target)},
                            ${templateEscape(memberName.dq())},
                            $idx,
                        )$memberTraitChain;
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
                    static ${schemaPrefix}_MEMBER: #{Schema} = #{Schema}::new_member(
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
                    static ${schemaPrefix}_KEY: #{Schema} = #{Schema}::new_member(
                        #{ShapeId}::from_static(
                            "$escapedKeyId",
                            "${shape.key.id.namespace}",
                            "${shape.key.id.name}",
                        ),
                        #{ShapeType}::${shapeTypeVariant(keyTarget)},
                        "key",
                        0,
                    );

                    static ${schemaPrefix}_VALUE: #{Schema} = #{Schema}::new_member(
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
