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
import software.amazon.smithy.model.shapes.IntEnumShape
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
import software.amazon.smithy.model.traits.SparseTrait
import software.amazon.smithy.model.traits.StreamingTrait
import software.amazon.smithy.model.traits.TimestampFormatTrait
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
import software.amazon.smithy.rust.codegen.core.util.isTargetUnit
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
 * Describes a synthetic member to add to a schema (e.g., `_request_id` from a response header).
 * These are not in the Smithy model but are added by SDK-specific decorators.
 */
data class SyntheticSchemaMember(
    /** The Rust field name on the builder (e.g., `_request_id`). */
    val fieldName: String,
    /** The Smithy member name for the schema (e.g., `requestId`). */
    val schemaMemberName: String,
    /** The shape type (e.g., `String`). */
    val shapeType: String,
    /** The HTTP header name to bind to (e.g., `x-amzn-requestid`). */
    val httpHeaderName: String,
)

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
    private val syntheticMembers: List<SyntheticSchemaMember> = emptyList(),
    /** Override the prefix used for generated static names. Defaults to the symbol name uppercased. */
    val schemaPrefix: String? = null,
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

    /** Renders only the schema statics (no impl blocks, no SerializableStruct, no deserialize). */
    fun renderSchemaOnly() {
        val symbol = symbolProvider.toSymbol(shape)
        val codegenScope =
            arrayOf(
                "Schema" to smithySchema.resolve("Schema"),
                "ShapeId" to smithySchema.resolve("ShapeId"),
                "ShapeType" to smithySchema.resolve("ShapeType"),
            )
        val schemaPrefix = this.schemaPrefix ?: symbol.name.uppercase()
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
        renderSchemaStatic(writer, schemaPrefix, symbol.name)
    }

    fun render() {
        val symbol = symbolProvider.toSymbol(shape)
        val codegenScope =
            arrayOf(
                "Schema" to smithySchema.resolve("Schema"),
                "ShapeId" to smithySchema.resolve("ShapeId"),
                "ShapeType" to smithySchema.resolve("ShapeType"),
            )

        val schemaPrefix = this.schemaPrefix ?: symbol.name.uppercase()

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

        // Write SerializableStruct impl for structures and unions
        if (shape is StructureShape) {
            renderSerializableStruct(writer, symbol.name, schemaPrefix)
            renderDeserializeMethod(writer, symbol.name, schemaPrefix)
            renderDeserializeHttpHeaders(writer, symbol.name, schemaPrefix)
        } else if (shape is UnionShape) {
            renderSerializableUnion(writer, symbol.name, schemaPrefix)
            renderDeserializeUnion(writer, symbol.name, schemaPrefix)
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
                    val target = model.expectShape(member.target)
                    // Skip streaming members (event streams, streaming blobs) — they are
                    // serialized by the protocol layer, not the codec.
                    if (target.hasTrait(StreamingTrait::class.java)) return@forEachIndexed
                    val memberName = symbolProvider.toMemberName(member)
                    val memberSymbol = symbolProvider.toSymbol(member)
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

    private fun renderSerializableUnion(
        writer: RustWriter,
        unionName: String,
        schemaPrefix: String,
    ) {
        val codegenScope =
            arrayOf(
                "SerializableStruct" to smithySchema.resolve("serde::SerializableStruct"),
                "ShapeSerializer" to smithySchema.resolve("serde::ShapeSerializer"),
                "SerdeError" to smithySchema.resolve("serde::SerdeError"),
            )
        val union = shape as UnionShape
        val members = union.allMembers.values.toList()

        val variantArms =
            writable {
                members.forEachIndexed { idx, member ->
                    val rustMemberName = symbolProvider.toMemberName(member)
                    val variantName = symbolProvider.toSymbol(member).name
                    val target = model.expectShape(member.target)
                    val memberSchemaRef = "${schemaPrefix}_MEMBER_${constantName(rustMemberName)}"

                    if (member.isTargetUnit()) {
                        // Unit variants serialize as empty objects {} in JSON, not null
                        rust(
                            """
                            Self::$variantName => {
                                struct Empty;
                                impl ::aws_smithy_schema::serde::SerializableStruct for Empty {
                                    fn serialize_members(&self, _ser: &mut dyn ::aws_smithy_schema::serde::ShapeSerializer) -> ::std::result::Result<(), ::aws_smithy_schema::serde::SerdeError> { Ok(()) }
                                }
                                ser.write_struct(&$memberSchemaRef, &Empty)?;
                            },
                            """,
                        )
                    } else {
                        val writeExpr = unionVariantWriteExpr(target, memberSchemaRef, "val")
                        rust("Self::$variantName(val) => { $writeExpr },")
                    }
                }
                rustTemplate("Self::${UnionGenerator.UNKNOWN_VARIANT_NAME} => return Err(#{SerdeError}::custom(\"cannot serialize unknown union variant\")),", *codegenScope)
            }

        writer.rustTemplate(
            """
            impl #{SerializableStruct} for $unionName {
                ##[allow(unused_variables, clippy::diverging_sub_expression)]
                fn serialize_members(&self, ser: &mut dyn #{ShapeSerializer}) -> ::std::result::Result<(), #{SerdeError}> {
                    match self {
                        #{variantArms}
                    }
                    Ok(())
                }
            }
            """,
            *codegenScope,
            "variantArms" to variantArms,
        )
    }

    /** Returns a write expression for a union variant value. */
    private fun unionVariantWriteExpr(
        target: Shape,
        memberSchemaRef: String,
        varName: String,
    ): String {
        return when (target) {
            is BooleanShape -> "ser.write_boolean(&$memberSchemaRef, *$varName)?;"
            is ByteShape -> "ser.write_byte(&$memberSchemaRef, *$varName)?;"
            is ShortShape -> "ser.write_short(&$memberSchemaRef, *$varName)?;"
            is IntegerShape -> "ser.write_integer(&$memberSchemaRef, *$varName)?;"
            is LongShape -> "ser.write_long(&$memberSchemaRef, *$varName)?;"
            is FloatShape -> "ser.write_float(&$memberSchemaRef, *$varName)?;"
            is DoubleShape -> "ser.write_double(&$memberSchemaRef, *$varName)?;"
            is BigIntegerShape -> "ser.write_big_integer(&$memberSchemaRef, $varName)?;"
            is BigDecimalShape -> "ser.write_big_decimal(&$memberSchemaRef, $varName)?;"
            is EnumShape -> "ser.write_string(&$memberSchemaRef, $varName.as_str())?;"
            is StringShape ->
                if (isStringEnum(target)) {
                    "ser.write_string(&$memberSchemaRef, $varName.as_str())?;"
                } else {
                    "ser.write_string(&$memberSchemaRef, $varName)?;"
                }
            is BlobShape -> "ser.write_blob(&$memberSchemaRef, $varName)?;"
            is TimestampShape -> "ser.write_timestamp(&$memberSchemaRef, $varName)?;"
            is StructureShape -> "ser.write_struct(&$memberSchemaRef, $varName)?;"
            is ListShape -> {
                val elementTarget = model.expectShape(target.member.target)
                // Use helpers for simple non-enum element types
                when (elementTarget) {
                    is StringShape -> if (!isStringEnum(elementTarget)) "ser.write_string_list(&$memberSchemaRef, $varName)?;" else null
                    is BlobShape -> "ser.write_blob_list(&$memberSchemaRef, $varName)?;"
                    is IntegerShape, is IntEnumShape -> "ser.write_integer_list(&$memberSchemaRef, $varName)?;"
                    is LongShape -> "ser.write_long_list(&$memberSchemaRef, $varName)?;"
                    else -> null
                } ?: run {
                    val elementWrite = elementWriteExpr(elementTarget, "item")
                    """
                    ser.write_list(&$memberSchemaRef, &|ser: &mut dyn ::aws_smithy_schema::serde::ShapeSerializer| {
                        for item in $varName {
                            $elementWrite
                        }
                        Ok(())
                    })?;
                    """
                }
            }
            is MapShape -> {
                val keyTarget = model.expectShape(target.key.target)
                val valueTarget = model.expectShape(target.value.target)
                // Use helper for non-enum string key, string value maps
                if (!isStringEnum(keyTarget) && valueTarget is StringShape && !isStringEnum(valueTarget)) {
                    "ser.write_string_string_map(&$memberSchemaRef, $varName)?;"
                } else {
                    val keyExpr = if (isStringEnum(keyTarget)) "key.as_str()" else "key"
                    val valueWrite = mapValueWriteExpr(valueTarget, "value")
                    """
                    ser.write_map(&$memberSchemaRef, &|ser: &mut dyn ::aws_smithy_schema::serde::ShapeSerializer| {
                        for (key, value) in $varName {
                            ser.write_string(&::aws_smithy_schema::prelude::STRING, $keyExpr)?;
                            $valueWrite
                        }
                        Ok(())
                    })?;
                    """
                }
            }
            is UnionShape -> "ser.write_struct(&$memberSchemaRef, $varName)?;"
            is DocumentShape -> "ser.write_document(&$memberSchemaRef, $varName)?;"
            else -> "todo!(\"schema: unsupported union variant type\");"
        }
    }

    private fun renderDeserializeUnion(
        writer: RustWriter,
        unionName: String,
        schemaPrefix: String,
    ) {
        val codegenScope =
            arrayOf(
                "ShapeDeserializer" to smithySchema.resolve("serde::ShapeDeserializer"),
                "SerdeError" to smithySchema.resolve("serde::SerdeError"),
            )
        val union = shape as UnionShape
        val members = union.allMembers.values.toList()

        val variantArms =
            writable {
                members.forEachIndexed { idx, member ->
                    val variantName = symbolProvider.toSymbol(member).name
                    val memberSymbol = symbolProvider.toSymbol(member)
                    val target = model.expectShape(member.target)
                    if (member.isTargetUnit()) {
                        rust("Some($idx) => { deser.read_struct(member, &mut |_, _| Ok(()))?; Self::$variantName },")
                    } else {
                        val readExpr = readMethodForShape(target, "member")
                        val wrapped = if (memberSymbol.isRustBoxed()) "Box::new($readExpr)" else readExpr
                        rust("Some($idx) => Self::$variantName($wrapped),")
                    }
                }
                rust("_ => Self::${UnionGenerator.UNKNOWN_VARIANT_NAME},")
            }

        writer.rustTemplate(
            """
            impl $unionName {
                /// Deserializes this union from a [`ShapeDeserializer`].
                pub fn deserialize(deserializer: &mut dyn #{ShapeDeserializer}) -> ::std::result::Result<Self, #{SerdeError}> {
                    let mut result: ::std::option::Option<Self> = ::std::option::Option::None;
                    ##[allow(unused_variables, unreachable_code, clippy::single_match, clippy::match_single_binding)]
                    deserializer.read_struct(&${schemaPrefix}_SCHEMA, &mut |member, deser| {
                        result = ::std::option::Option::Some(match member.member_index() {
                            #{variantArms}
                        });
                        Ok(())
                    })?;
                    result.ok_or_else(|| #{SerdeError}::custom("expected a union variant"))
                }
            }
            """,
            *codegenScope,
            "variantArms" to variantArms,
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
            is UnionShape -> "ser.write_struct(&$memberSchemaRef, val)?;"
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
            is DocumentShape -> "ser.write_document(&$prelude::DOCUMENT, $varName)?;"
            is StructureShape -> {
                val targetQualified = symbolProvider.toSymbol(target).rustType().qualifiedName()
                "ser.write_struct($targetQualified::SCHEMA, $varName)?;"
            }

            is MapShape -> {
                val keyTarget = model.expectShape(target.key.target)
                val keyExpr = if (isStringEnum(keyTarget)) "key.as_str()" else "key"
                val valueTarget = model.expectShape(target.value.target)
                val valueWrite = mapValueWriteExpr(valueTarget, "value")
                val isSparse = target.hasTrait(SparseTrait::class.java)
                val targetQualified = symbolProvider.toSymbol(target).rustType().qualifiedName()
                if (isSparse) {
                    """
                    ser.write_map(&::aws_smithy_schema::prelude::DOCUMENT, &|ser: &mut dyn ::aws_smithy_schema::serde::ShapeSerializer| {
                        for (key, value) in $varName {
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
                    ser.write_map(&::aws_smithy_schema::prelude::DOCUMENT, &|ser: &mut dyn ::aws_smithy_schema::serde::ShapeSerializer| {
                        for (key, value) in $varName {
                            ser.write_string(&::aws_smithy_schema::prelude::STRING, $keyExpr)?;
                            $valueWrite
                        }
                        Ok(())
                    })?;
                    """
                }
            }

            is ListShape -> {
                val elementTarget = model.expectShape(target.member.target)
                val elementWrite = elementWriteExpr(elementTarget, "item")
                """
                ser.write_list(&::aws_smithy_schema::prelude::DOCUMENT, &|ser: &mut dyn ::aws_smithy_schema::serde::ShapeSerializer| {
                    for item in $varName {
                        $elementWrite
                    }
                    Ok(())
                })?;
                """
            }

            is UnionShape -> {
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
            is DocumentShape -> "ser.write_document(&$prelude::DOCUMENT, $varName)?;"
            is StructureShape -> {
                val targetQualified = symbolProvider.toSymbol(target).rustType().qualifiedName()
                "ser.write_struct($targetQualified::SCHEMA, $varName)?;"
            }

            is MapShape -> {
                val keyTarget = model.expectShape(target.key.target)
                val keyExpr = if (isStringEnum(keyTarget)) "key.as_str()" else "key"
                val valueTarget = model.expectShape(target.value.target)
                val innerValueWrite = mapValueWriteExpr(valueTarget, "value")
                val isSparse = target.hasTrait(SparseTrait::class.java)
                if (isSparse) {
                    """
                    ser.write_map(&$prelude::DOCUMENT, &|ser: &mut dyn ::aws_smithy_schema::serde::ShapeSerializer| {
                        for (key, value) in $varName {
                            ser.write_string(&$prelude::STRING, $keyExpr)?;
                            match value {
                                Some(value) => { $innerValueWrite }
                                None => { ser.write_null(&$prelude::STRING)?; }
                            }
                        }
                        Ok(())
                    })?;
                    """
                } else {
                    """
                    ser.write_map(&$prelude::DOCUMENT, &|ser: &mut dyn ::aws_smithy_schema::serde::ShapeSerializer| {
                        for (key, value) in $varName {
                            ser.write_string(&$prelude::STRING, $keyExpr)?;
                            $innerValueWrite
                        }
                        Ok(())
                    })?;
                    """
                }
            }

            is ListShape -> {
                val elementTarget = model.expectShape(target.member.target)
                val elementWrite = elementWriteExpr(elementTarget, "item")
                """
                ser.write_list(&$prelude::DOCUMENT, &|ser: &mut dyn ::aws_smithy_schema::serde::ShapeSerializer| {
                    for item in $varName {
                        $elementWrite
                    }
                    Ok(())
                })?;
                """
            }

            is UnionShape -> {
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
            )
        val members = (shape as StructureShape).allMembers.values.toList()

        writer.rustTemplate(
            """
            impl $structName {
                /// Deserializes this structure from a [`ShapeDeserializer`].
                pub fn deserialize(deserializer: &mut dyn #{ShapeDeserializer}) -> ::std::result::Result<Self, #{SerdeError}> {
                    ##[allow(unused_variables, unused_mut)]
                    let mut builder = Self::builder();
                    ##[allow(unused_variables, unreachable_code, clippy::single_match, clippy::match_single_binding, clippy::diverging_sub_expression)]
                    deserializer.read_struct(&${schemaPrefix}_SCHEMA, &mut |member, deser| {
                        match member.member_index() {
                            #{memberArms}
                            _ => {}
                        }
                        Ok(())
                    })?;
                    #{buildExpr}
                }
            }
            """,
            *codegenScope,
            "buildExpr" to
                writable {
                    if (BuilderGenerator.hasFallibleBuilder(shape as StructureShape, symbolProvider)) {
                        // Set defaults for required members that are still None (error correction).
                        // Only for types where we know a safe default value.
                        for (member in members) {
                            if (member.isRequired) {
                                val memberName = symbolProvider.toMemberName(member)
                                val target = model.expectShape(member.target)
                                val defaultExpr =
                                    when (target) {
                                        is StringShape -> if (target is EnumShape || target.hasTrait(EnumTrait::class.java)) null else "String::new()"
                                        is BooleanShape -> "false"
                                        is ByteShape -> "0i8"
                                        is ShortShape -> "0i16"
                                        is IntegerShape, is IntEnumShape -> "0i32"
                                        is LongShape -> "0i64"
                                        is FloatShape -> "0.0f32"
                                        is DoubleShape -> "0.0f64"
                                        is ListShape -> "Vec::new()"
                                        is MapShape -> "::std::collections::HashMap::new()"
                                        is BlobShape -> if (target.hasTrait(software.amazon.smithy.model.traits.StreamingTrait::class.java)) null else "::aws_smithy_types::Blob::new(\"\")"
                                        is TimestampShape -> "::aws_smithy_types::DateTime::from_secs(0)"
                                        else -> null
                                    }
                                if (defaultExpr != null) {
                                    rust("builder.$memberName = builder.$memberName.or(Some($defaultExpr));")
                                }
                            }
                        }
                        rust("builder.build().map_err(|e| aws_smithy_schema::serde::SerdeError::Custom { message: e.to_string() })")
                    } else {
                        rust("Ok(builder.build())")
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
                        if (memberSymbol.isOptional()) {
                            rust(
                                """
                                Some($idx) => {
                                    builder.$memberName = Some($wrapped);
                                }
                                """,
                            )
                        } else {
                            rust("Some($idx) => { builder.$memberName = Some($wrapped); }")
                        }
                    }
                    // Synthetic members (e.g., _request_id from response headers)
                    val baseIndex = members.size
                    syntheticMembers.forEachIndexed { i, synth ->
                        val synthIdx = baseIndex + i
                        rust(
                            """
                            Some($synthIdx) => {
                                builder.${synth.fieldName} = Some(deser.read_string(member)?);
                            }
                            """,
                        )
                    }
                },
        )
    }

    /**
     * Generates a `deserialize_http_headers` method on the output type that reads
     * `@httpHeader`, `@httpResponseCode`, and `@httpPrefixHeaders` members directly
     * from the HTTP response. This is called by the generated `deserialize_nonstreaming`
     * before body deserialization, avoiding the runtime member iteration overhead in
     * `HttpBindingDeserializer::read_struct`.
     *
     * Only generated if the struct has at least one HTTP response binding.
     */
    private fun renderDeserializeHttpHeaders(
        writer: RustWriter,
        structName: String,
        schemaPrefix: String,
    ) {
        val structShape = shape as StructureShape
        val members = structShape.allMembers.values.toList()

        data class HeaderMember(val memberName: String, val headerName: String, val isBool: Boolean, val target: Shape?, val member: MemberShape? = null, val hasMediaType: Boolean = false)

        data class StatusMember(val memberName: String)

        data class PrefixMember(val memberName: String, val prefix: String)

        val headerMembers = mutableListOf<HeaderMember>()
        var statusMember: StatusMember? = null
        var prefixMember: PrefixMember? = null

        for (member in members) {
            val memberName = symbolProvider.toMemberName(member)
            val httpHeader = member.getTrait(software.amazon.smithy.model.traits.HttpHeaderTrait::class.java)
            val httpResponseCode = member.getTrait(software.amazon.smithy.model.traits.HttpResponseCodeTrait::class.java)
            val httpPrefixHeaders = member.getTrait(software.amazon.smithy.model.traits.HttpPrefixHeadersTrait::class.java)
            val target = model.expectShape(member.target)

            if (httpHeader.isPresent) {
                val hasMediaType =
                    target.hasTrait(software.amazon.smithy.model.traits.MediaTypeTrait::class.java) ||
                        member.hasTrait(software.amazon.smithy.model.traits.MediaTypeTrait::class.java)
                headerMembers.add(HeaderMember(memberName, httpHeader.get().value, target is BooleanShape, target, member, hasMediaType))
            } else if (httpResponseCode.isPresent) {
                statusMember = StatusMember(memberName)
            } else if (httpPrefixHeaders.isPresent) {
                prefixMember = PrefixMember(memberName, httpPrefixHeaders.get().value)
            }
        }

        // Also check synthetic members
        for (synth in syntheticMembers) {
            headerMembers.add(HeaderMember(synth.fieldName, synth.httpHeaderName, false, null))
        }

        // Detect @httpPayload member early — needed for both early-return and main paths
        val httpPayloadMember =
            structShape.allMembers.values.firstOrNull {
                it.hasTrait(software.amazon.smithy.model.traits.HttpPayloadTrait::class.java)
            }
        val payloadTarget = httpPayloadMember?.let { model.expectShape(it.target) }
        val isRawPayload =
            (payloadTarget is BlobShape || payloadTarget is StringShape) &&
                payloadTarget?.getTrait(software.amazon.smithy.model.traits.StreamingTrait::class.java)?.isPresent != true
        val isStructPayload =
            (payloadTarget is StructureShape || payloadTarget is UnionShape) &&
                payloadTarget?.getTrait(software.amazon.smithy.model.traits.StreamingTrait::class.java)?.isPresent != true
        val isDocumentPayload = payloadTarget is DocumentShape
        val hasPayloadHandling = isRawPayload || isStructPayload || isDocumentPayload

        if (headerMembers.isEmpty() && statusMember == null && prefixMember == null && !hasPayloadHandling) {
            // No HTTP-bound members and no @httpPayload — generate a trivial deserialize_with_response
            // that delegates to deserialize.
            writer.rustTemplate(
                """
                impl $structName {
                    /// Deserializes this structure from a body deserializer and HTTP response.
                    pub fn deserialize_with_response(
                        deserializer: &mut dyn #{ShapeDeserializer},
                        _headers: &#{Headers},
                        _status: u16,
                        _body: &[u8],
                    ) -> ::std::result::Result<Self, #{SerdeError}> {
                        Self::deserialize(deserializer)
                    }
                }
                """,
                "ShapeDeserializer" to smithySchema.resolve("serde::ShapeDeserializer"),
                "SerdeError" to smithySchema.resolve("serde::SerdeError"),
                "Headers" to RuntimeType.smithyRuntimeApi(runtimeConfig).resolve("http::Headers"),
            )
            return
        }

        val headersParam = if (headerMembers.isNotEmpty() || prefixMember != null) "headers" else "_headers"
        val bodyParam = if (hasPayloadHandling) "body" else "_body"
        val deserializerParam = if (isRawPayload) "_deserializer" else "deserializer"

        writer.rustTemplate(
            """
            impl $structName {
                /// Deserializes this structure from a body deserializer and HTTP response headers.
                /// Header-bound members are read directly from headers, avoiding runtime
                /// member iteration overhead. Body members are read via the deserializer.
                pub fn deserialize_with_response(
                    $deserializerParam: &mut dyn #{ShapeDeserializer},
                    $headersParam: &#{Headers},
                    _status: u16,
                    $bodyParam: &[u8],
                ) -> ::std::result::Result<Self, #{SerdeError}> {
                    ##[allow(unused_variables, unused_mut)]
                    let mut builder = Self::builder();
            """,
            "ShapeDeserializer" to smithySchema.resolve("serde::ShapeDeserializer"),
            "SerdeError" to smithySchema.resolve("serde::SerdeError"),
            "Headers" to RuntimeType.smithyRuntimeApi(runtimeConfig).resolve("http::Headers"),
        )

        // Read headers directly
        for (hm in headerMembers) {
            val parseExpr =
                when (hm.target) {
                    is BooleanShape -> "val.parse::<bool>().ok()"
                    is ByteShape -> "val.parse::<i8>().ok()"
                    is ShortShape -> "val.parse::<i16>().ok()"
                    is IntegerShape -> "val.parse::<i32>().ok()"
                    is LongShape -> "val.parse::<i64>().ok()"
                    is FloatShape -> "val.parse::<f32>().ok()"
                    is DoubleShape -> "val.parse::<f64>().ok()"
                    is TimestampShape -> {
                        // Check @timestampFormat on member or target; default to HttpDate for headers
                        val tsFormatOpt =
                            hm.member?.getTrait(TimestampFormatTrait::class.java)
                                ?.let { if (it.isPresent) it else hm.target?.getTrait(TimestampFormatTrait::class.java) }
                                ?: hm.target?.getTrait(TimestampFormatTrait::class.java)
                        val format =
                            if (tsFormatOpt?.isPresent == true) {
                                when (tsFormatOpt.get().format.toString()) {
                                    "epoch-seconds" -> "EpochSeconds"
                                    "date-time" -> "DateTime"
                                    else -> "HttpDate"
                                }
                            } else {
                                "HttpDate"
                            }
                        if (format == "EpochSeconds") {
                            "val.parse::<f64>().ok().map(::aws_smithy_types::DateTime::from_secs_f64)"
                        } else {
                            "::aws_smithy_types::DateTime::from_str(val, ::aws_smithy_types::date_time::Format::$format).ok()"
                        }
                    }
                    is EnumShape -> {
                        val enumName = symbolProvider.toSymbol(hm.target).rustType().qualifiedName()
                        "Some($enumName::from(val))"
                    }
                    is IntEnumShape -> {
                        val enumName = symbolProvider.toSymbol(hm.target).rustType().qualifiedName()
                        "val.parse::<i32>().ok().map($enumName::from)"
                    }
                    is StringShape -> {
                        if (hm.hasMediaType) {
                            // @mediaType on header: base64-decode the value
                            "::aws_smithy_types::base64::decode(val).ok().and_then(|b| String::from_utf8(b).ok())"
                        } else if (hm.target.hasTrait(EnumTrait::class.java)) {
                            val enumName = symbolProvider.toSymbol(hm.target).rustType().qualifiedName()
                            "Some($enumName::from(val))"
                        } else {
                            "Some(val.to_string())"
                        }
                    }
                    is ListShape -> {
                        val elementTarget = model.expectShape((hm.target as ListShape).member.target)
                        if (elementTarget is TimestampShape) {
                            // HTTP-date contains commas — split on ", " followed by day-of-week
                            val listMember = (hm.target as ListShape).member
                            val tsFormatOpt =
                                listMember.getTrait(TimestampFormatTrait::class.java)
                                    .let { if (it.isPresent) it else elementTarget.getTrait(TimestampFormatTrait::class.java) }
                            val format =
                                if (tsFormatOpt.isPresent) {
                                    when (tsFormatOpt.get().format.toString()) {
                                        "epoch-seconds" -> "EpochSeconds"
                                        "date-time" -> "DateTime"
                                        else -> "HttpDate"
                                    }
                                } else {
                                    "HttpDate"
                                }
                            if (format == "HttpDate") {
                                // HTTP-date values are separated by ", " but also contain internal commas.
                                // Each HTTP-date is exactly 29 chars. Split by regex for day-of-week boundary.
                                """
                                {
                                    let mut timestamps = Vec::new();
                                    let re_split: Vec<&str> = val.split(", ").collect();
                                    let mut i = 0;
                                    while i < re_split.len() {
                                        if i + 1 < re_split.len() {
                                            let combined = format!("{}, {}", re_split[i], re_split[i + 1]);
                                            if let Ok(ts) = ::aws_smithy_types::DateTime::from_str(&combined, ::aws_smithy_types::date_time::Format::HttpDate) {
                                                timestamps.push(ts);
                                                i += 2;
                                                continue;
                                            }
                                        }
                                        if let Ok(ts) = ::aws_smithy_types::DateTime::from_str(re_split[i].trim(), ::aws_smithy_types::date_time::Format::HttpDate) {
                                            timestamps.push(ts);
                                        }
                                        i += 1;
                                    }
                                    Some(timestamps)
                                }
                                """.trimIndent()
                            } else if (format == "EpochSeconds") {
                                "Some(val.split(',').filter_map(|s| s.trim().parse::<f64>().ok().map(::aws_smithy_types::DateTime::from_secs_f64)).collect())"
                            } else {
                                "Some(val.split(',').filter_map(|s| ::aws_smithy_types::DateTime::from_str(s.trim(), ::aws_smithy_types::date_time::Format::$format).ok()).collect())"
                            }
                        } else {
                            val isPlainString = elementTarget is StringShape && !elementTarget.hasTrait(EnumTrait::class.java) && elementTarget !is EnumShape
                            if (isPlainString) {
                                // String lists need quoted-string-aware parsing (RFC 7230)
                                """
                                {
                                    let mut items = Vec::new();
                                    let mut chars = val.chars().peekable();
                                    while chars.peek().is_some() {
                                        // Skip whitespace
                                        while chars.peek() == Some(&' ') { chars.next(); }
                                        if chars.peek() == Some(&'"') {
                                            chars.next(); // skip opening quote
                                            let mut s = String::new();
                                            while let Some(&c) = chars.peek() {
                                                if c == '\\' { chars.next(); if let Some(escaped) = chars.next() { s.push(escaped); } }
                                                else if c == '"' { chars.next(); break; }
                                                else { s.push(c); chars.next(); }
                                            }
                                            items.push(s);
                                        } else {
                                            let s: String = chars.by_ref().take_while(|&c| c != ',').collect();
                                            let trimmed = s.trim();
                                            if !trimmed.is_empty() { items.push(trimmed.to_string()); }
                                        }
                                        // Skip comma separator
                                        while chars.peek() == Some(&',') || chars.peek() == Some(&' ') { chars.next(); }
                                    }
                                    Some(items)
                                }
                                """.trimIndent()
                            } else {
                                val mapExpr =
                                    when {
                                        elementTarget is EnumShape -> {
                                            val enumName = symbolProvider.toSymbol(elementTarget).rustType().qualifiedName()
                                            ".map(|s| $enumName::from(s.trim()))"
                                        }
                                        elementTarget is StringShape && elementTarget.hasTrait(EnumTrait::class.java) -> {
                                            val enumName = symbolProvider.toSymbol(elementTarget).rustType().qualifiedName()
                                            ".map(|s| $enumName::from(s.trim()))"
                                        }
                                        elementTarget is BooleanShape -> ".filter_map(|s| s.trim().parse::<bool>().ok())"
                                        elementTarget is ByteShape -> ".filter_map(|s| s.trim().parse::<i8>().ok())"
                                        elementTarget is ShortShape -> ".filter_map(|s| s.trim().parse::<i16>().ok())"
                                        elementTarget is IntegerShape -> ".filter_map(|s| s.trim().parse::<i32>().ok())"
                                        elementTarget is LongShape -> ".filter_map(|s| s.trim().parse::<i64>().ok())"
                                        elementTarget is FloatShape -> ".filter_map(|s| s.trim().parse::<f32>().ok())"
                                        elementTarget is DoubleShape -> ".filter_map(|s| s.trim().parse::<f64>().ok())"
                                        else -> ".map(|s| s.trim().to_string())"
                                    }
                                "Some(val.split(',')$mapExpr.collect())"
                            }
                        }
                    }
                    else -> "Some(val.to_string())"
                }
            writer.rust(
                """
                if let Some(val) = headers.get(${hm.headerName.dq()}) {
                    builder.${hm.memberName} = $parseExpr;
                }
                """,
            )
        }

        if (statusMember != null) {
            writer.rust("builder.${statusMember.memberName} = Some(_status as i32);")
        }

        if (prefixMember != null) {
            writer.rust(
                """
                {
                    let mut map = ::std::collections::HashMap::new();
                    for (key, val) in headers.iter() {
                        if let Some(suffix) = key.strip_prefix(${prefixMember.prefix.dq()}) {
                            map.insert(suffix.to_string(), val.to_string());
                        }
                    }
                    if !map.is_empty() {
                        builder.${prefixMember.memberName} = Some(map);
                    }
                }
                """,
            )
        }

        // @httpPayload handling — read body directly (variables detected earlier)
        if (isStructPayload && httpPayloadMember != null) {
            // @httpPayload struct/union: deserialize body directly as the target type
            val memberName = symbolProvider.toMemberName(httpPayloadMember)
            val targetQualified = symbolProvider.toSymbol(payloadTarget!!).rustType().qualifiedName()
            writer.rust(
                """
                if !body.is_empty() {
                    builder.$memberName = Some($targetQualified::deserialize(deserializer)?);
                }
                """,
            )
            // Build the output
            writer.rustTemplate(
                """
                #{buildExpr}
                }
                }
                """,
                "buildExpr" to
                    writable {
                        if (BuilderGenerator.hasFallibleBuilder(structShape, symbolProvider)) {
                            rust("builder.build().map_err(|e| aws_smithy_schema::serde::SerdeError::Custom { message: e.to_string() })")
                        } else {
                            rust("Ok(builder.build())")
                        }
                    },
            )
        } else if (isRawPayload && httpPayloadMember != null) {
            val memberName = symbolProvider.toMemberName(httpPayloadMember)
            if (payloadTarget is BlobShape) {
                writer.rust(
                    """
                    if !body.is_empty() {
                        builder.$memberName = Some(::aws_smithy_types::Blob::new(body.to_vec()));
                    }
                    """,
                )
            } else {
                // String or enum payload — read body as UTF-8 string
                val targetQualified =
                    if (payloadTarget is EnumShape || payloadTarget!!.hasTrait(EnumTrait::class.java)) {
                        val enumName = symbolProvider.toSymbol(payloadTarget).rustType().qualifiedName()
                        "$enumName::from(s.as_str())"
                    } else {
                        "s"
                    }
                writer.rust(
                    """
                    if !body.is_empty() {
                        let s = ::std::string::String::from_utf8_lossy(body).into_owned();
                        builder.$memberName = Some($targetQualified);
                    }
                    """,
                )
            }
            // Build the output
            writer.rustTemplate(
                """
                #{buildExpr}
                }
                }
                """,
                "buildExpr" to
                    writable {
                        if (BuilderGenerator.hasFallibleBuilder(structShape, symbolProvider)) {
                            rust("builder.build().map_err(|e| aws_smithy_schema::serde::SerdeError::Custom { message: e.to_string() })")
                        } else {
                            rust("Ok(builder.build())")
                        }
                    },
            )
        } else if (isDocumentPayload && httpPayloadMember != null) {
            val memberName = symbolProvider.toMemberName(httpPayloadMember)
            val memberSchemaRef = "${schemaPrefix}_MEMBER_${constantName(memberName)}"
            writer.rust(
                """
                if !body.is_empty() {
                    builder.$memberName = Some(deserializer.read_document(&$memberSchemaRef)?);
                }
                """,
            )
            // Build the output
            writer.rustTemplate(
                """
                #{buildExpr}
                }
                }
                """,
                "buildExpr" to
                    writable {
                        if (BuilderGenerator.hasFallibleBuilder(structShape, symbolProvider)) {
                            rust("builder.build().map_err(|e| aws_smithy_schema::serde::SerdeError::Custom { message: e.to_string() })")
                        } else {
                            rust("Ok(builder.build())")
                        }
                    },
            )
        } else {
            // Now deserialize body members
            writer.rustTemplate(
                """
                ##[allow(unused_variables, unreachable_code, clippy::single_match, clippy::match_single_binding, clippy::diverging_sub_expression)]
                deserializer.read_struct(&${schemaPrefix}_SCHEMA, &mut |member, deser| {
                    match member.member_index() {
                        #{memberArms}
                        _ => {}
                    }
                    Ok(())
                })?;
                #{buildExpr}
                }
                }
                """,
                "ShapeDeserializer" to smithySchema.resolve("serde::ShapeDeserializer"),
                "SerdeError" to smithySchema.resolve("serde::SerdeError"),
                "buildExpr" to
                    writable {
                        if (BuilderGenerator.hasFallibleBuilder(structShape, symbolProvider)) {
                            rust("builder.build().map_err(|e| aws_smithy_schema::serde::SerdeError::Custom { message: e.to_string() })")
                        } else {
                            rust("Ok(builder.build())")
                        }
                    },
                "memberArms" to
                    writable {
                        val allMembers = structShape.allMembers.values.toList()
                        allMembers.forEachIndexed { idx, member ->
                            val memberName = symbolProvider.toMemberName(member)
                            val memberSymbol = symbolProvider.toSymbol(member)
                            val target = model.expectShape(member.target)
                            // Skip HTTP-bound members — they're already set from headers above
                            val hasHttpBinding =
                                member.getTrait(software.amazon.smithy.model.traits.HttpHeaderTrait::class.java).isPresent ||
                                    member.getTrait(software.amazon.smithy.model.traits.HttpResponseCodeTrait::class.java).isPresent ||
                                    member.getTrait(software.amazon.smithy.model.traits.HttpPrefixHeadersTrait::class.java).isPresent
                            if (hasHttpBinding) {
                                rust("Some($idx) => { /* read from headers above */ }")
                            } else {
                                val readExpr = readMethodForShape(target, "member")
                                val wrapped = if (memberSymbol.isRustBoxed()) "Box::new($readExpr)" else readExpr
                                if (memberSymbol.isOptional()) {
                                    rust("Some($idx) => { builder.$memberName = Some($wrapped); }")
                                } else {
                                    rust("Some($idx) => { builder.$memberName = Some($wrapped); }")
                                }
                            }
                        }
                    },
            )
        } // end else (non-raw-payload path)
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
                // Use helper methods for common non-sparse simple-element lists
                val helperExpr =
                    if (!isSparse) {
                        when (elementTarget) {
                            is StringShape -> if (!isStringEnum(elementTarget)) "deser.read_string_list($memberRef)?" else null
                            is BlobShape -> "deser.read_blob_list($memberRef)?"
                            is IntegerShape, is IntEnumShape -> "deser.read_integer_list($memberRef)?"
                            is LongShape -> "deser.read_long_list($memberRef)?"
                            else -> null
                        }
                    } else {
                        null
                    }
                if (helperExpr != null) {
                    helperExpr
                } else {
                    val elementRead = elementReadExpr(elementTarget, memberRef)
                    val pushExpr =
                        if (isSparse) {
                            "container.push(if deser.is_null() { deser.read_null()?; None } else { Some($elementRead) })"
                        } else {
                            "container.push($elementRead)"
                        }
                    "{ let mut container = Vec::new(); deser.read_list($memberRef, &mut |deser| { $pushExpr; Ok(()) })?; container }"
                }
            }

            is MapShape -> {
                val isSparse = target.hasTrait(SparseTrait::class.java)
                val keyTarget = model.expectShape(target.key.target)
                val valueTarget = model.expectShape(target.value.target)
                // Use helper for non-sparse, plain string key, string value maps
                if (!isSparse && !isStringEnum(keyTarget) && valueTarget is StringShape && !isStringEnum(valueTarget)) {
                    "deser.read_string_string_map($memberRef)?"
                } else {
                    val keyInsert =
                        if (isStringEnum(keyTarget)) {
                            val enumName = symbolProvider.toSymbol(keyTarget).rustType().qualifiedName()
                            "$enumName::from(key.as_str())"
                        } else {
                            "key"
                        }
                    val valueRead = elementReadExpr(valueTarget, memberRef)
                    val insertExpr =
                        if (isSparse) {
                            "container.insert($keyInsert, if deser.is_null() { deser.read_null()?; None } else { Some($valueRead) })"
                        } else {
                            "container.insert($keyInsert, $valueRead)"
                        }
                    "{ let mut container = std::collections::HashMap::new(); deser.read_map($memberRef, &mut |key, deser| { $insertExpr; Ok(()) })?; container }"
                }
            }

            is StructureShape -> {
                val targetSymbol = symbolProvider.toSymbol(target)
                "${targetSymbol.rustType().qualifiedName()}::deserialize(deser)?"
            }

            is UnionShape -> {
                if (target.hasTrait(StreamingTrait::class.java)) {
                    "{ let _ = $memberRef; todo!(\"deserialize streaming union\") }"
                } else {
                    val targetSymbol = symbolProvider.toSymbol(target)
                    "${targetSymbol.rustType().qualifiedName()}::deserialize(deser)?"
                }
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
            is StructureShape -> {
                val targetSymbol = symbolProvider.toSymbol(target)
                "${targetSymbol.rustType().qualifiedName()}::deserialize(deser)?"
            }

            is UnionShape -> {
                val targetSymbol = symbolProvider.toSymbol(target)
                "${targetSymbol.rustType().qualifiedName()}::deserialize(deser)?"
            }

            is ListShape -> {
                val elementTarget = model.expectShape(target.member.target)
                val elementRead = elementReadExpr(elementTarget, "&::aws_smithy_schema::prelude::DOCUMENT")
                """
                {
                    let mut list = Vec::new();
                    deser.read_list(member, &mut |deser| {
                        list.push($elementRead);
                        Ok(())
                    })?;
                    list
                }
                """.trimIndent()
            }

            is MapShape -> {
                val valueTarget = model.expectShape(target.value.target)
                val valueRead = elementReadExpr(valueTarget, "&::aws_smithy_schema::prelude::DOCUMENT")
                """
                {
                    let mut map = ::std::collections::HashMap::new();
                    deser.read_map(member, &mut |key, deser| {
                        let value = $valueRead;
                        map.insert(key, value);
                        Ok(())
                    })?;
                    map
                }
                """.trimIndent()
            }

            else -> "todo!(\"deserialize nested aggregate\")"
        }

    /** Returns a Rust default value expression for a shape, or null if no sensible default exists. */
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

    /**
     * If this shape is an operation input, returns a `.with_http(...)` chain
     * for the operation's `@http` trait. The `@http` trait is operation-level
     * but is included on the input schema for convenience so the protocol
     * serializer can construct the request URI.
     */
    private fun httpTraitChain(shape: Shape): String {
        val operationIndex = software.amazon.smithy.model.knowledge.OperationIndex.of(model)
        for (operation in model.operationShapes) {
            if (operationIndex.getInputShape(operation).orElse(null)?.id == shape.id) {
                val httpTrait =
                    operation.getTrait(software.amazon.smithy.model.traits.HttpTrait::class.java).orElse(null)
                        ?: return ""
                val method = httpTrait.method.dq()
                val uri = httpTrait.uri.toString().dq()
                val code = httpTrait.code
                return "\n    .with_http(aws_smithy_schema::traits::HttpTrait::new($method, $uri, ${if (code == 200) "None" else "Some($code)"}))"
            }
        }
        return ""
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
                val modelRefs =
                    members.map { member ->
                        val memberName = symbolProvider.toMemberName(member)
                        "&${schemaPrefix}_MEMBER_${constantName(memberName)}"
                    }
                val synthRefs =
                    syntheticMembers.map { synth ->
                        "&${schemaPrefix}_MEMBER_${constantName(synth.fieldName)}"
                    }
                val allRefs = modelRefs + synthRefs
                val membersArray =
                    if (allRefs.isEmpty()) {
                        "&[]"
                    } else {
                        "&[${allRefs.joinToString(", ")}]"
                    }
                val traitChain = traitSetterChain(shape) + httpTraitChain(shape)
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
                    val rustMemberName = symbolProvider.toMemberName(member)
                    val smithyMemberName = member.memberName
                    val target = model.expectShape(member.target)
                    val escapedMemberId = member.id.toString().replace("#", "##")
                    val memberTraitChain = traitSetterChain(member)
                    // Include @timestampFormat from the target shape if not already on the member
                    val targetTimestampFormat =
                        if (
                            target is software.amazon.smithy.model.shapes.TimestampShape &&
                            !member.hasTrait(TimestampFormatTrait::class.java) &&
                            target.hasTrait(TimestampFormatTrait::class.java)
                        ) {
                            knownTraitSetter(target.expectTrait(TimestampFormatTrait::class.java)) ?: ""
                        } else {
                            ""
                        }
                    writer.rustTemplate(
                        """
                        static ${schemaPrefix}_MEMBER_${constantName(rustMemberName)}: #{Schema} = #{Schema}::new_member(
                            #{ShapeId}::from_static(
                                "$escapedMemberId",
                                "${member.id.namespace}",
                                "${member.id.name}",
                            ),
                            #{ShapeType}::${shapeTypeVariant(target)},
                            ${templateEscape(smithyMemberName.dq())},
                            $idx,
                        )$memberTraitChain$targetTimestampFormat;
                        """,
                        *codegenScope,
                    )
                }
                // Render synthetic members (e.g., _request_id from response headers)
                val baseIndex = shape.members().size
                syntheticMembers.forEachIndexed { i, synth ->
                    val synthIdx = baseIndex + i
                    writer.rustTemplate(
                        """
                        static ${schemaPrefix}_MEMBER_${constantName(synth.fieldName)}: #{Schema} = #{Schema}::new_member(
                            #{ShapeId}::from_static(
                                "synthetic##${synth.schemaMemberName}",
                                "synthetic",
                                "${synth.schemaMemberName}",
                            ),
                            #{ShapeType}::${synth.shapeType},
                            ${synth.schemaMemberName.dq()},
                            $synthIdx,
                        ).with_http_header(${synth.httpHeaderName.dq()});
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
