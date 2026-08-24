/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators


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
import software.amazon.smithy.model.traits.Trait as SmithyTrait
import software.amazon.smithy.model.traits.XmlNamespaceTrait
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.qualifiedName
import software.amazon.smithy.rust.codegen.core.rustlang.render
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.stripOuter
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.generators.RecursiveShapeClassifier
import software.amazon.smithy.rust.codegen.core.smithy.generators.SchemaTraitExtension
import software.amazon.smithy.rust.codegen.core.smithy.generators.SchemaTraitFilter
import software.amazon.smithy.rust.codegen.core.smithy.generators.isAnnotationTrait
import software.amazon.smithy.rust.codegen.core.smithy.generators.stringValue
import software.amazon.smithy.rust.codegen.core.smithy.generators.SyntheticSchemaMember
import software.amazon.smithy.rust.codegen.core.smithy.generators.UnionGenerator
import software.amazon.smithy.rust.codegen.core.smithy.isOptional
import software.amazon.smithy.rust.codegen.core.smithy.isRustBoxed
import software.amazon.smithy.rust.codegen.core.smithy.rustType
import software.amazon.smithy.rust.codegen.core.smithy.traits.SyntheticInputTrait
import software.amazon.smithy.rust.codegen.core.smithy.traits.SyntheticOutputTrait
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.isStreaming
import software.amazon.smithy.rust.codegen.core.util.isTargetUnit

/**
 * Server-side copy of [software.amazon.smithy.rust.codegen.core.smithy.generators.SchemaGenerator]
 * trimmed to the SERIALIZE-ONLY surface the schema-decoupled error path needs
 * (`renderSerializeOnly`), with the server-specific behaviors applied:
 *
 * - **Canonical model member order** in `serialize_members` (plan 2e): no
 *   protocol-specific ordering exists in generated code. RPC protocols match
 *   legacy byte-for-byte; restJson1 error bodies are gated parse-equal.
 * - **Constrained-string newtypes** (`publicConstrainedTypes=true`): members
 *   whose resolved Rust type is a string newtype serialize via `as_str()`.
 * - **No `Unknown` union arm**: server unions are generated with
 *   `renderUnknownVariant = false`.
 * - No deserialization methods are rendered (the deserialize methods in the
 *   core generator assume client-side builder conventions).
 *
 * A copy rather than a subclass because the core class is final with private
 * members, and the core file is imported verbatim from smithy-rs#4721 — it
 * must stay pristine so the eventual rebase (once #4721 merges) can drop it
 * cleanly.
 */
class ServerSchemaGenerator(
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

    // Used to decide whether a nested aggregate target reaches back to its
    // containing aggregate (a true recursive cycle in the schema graph).
    // For non-recursive cases the runtime serializer emissions can reference
    // the resolved sub-schema (`<PARENT>_MEMBER` / `<PARENT>_VALUE`) instead
    // of `prelude::DOCUMENT`, letting the codec see the inner aggregate's
    // member traits (e.g. `@xmlName` on map keys/values).
    private val recursiveClassifier = RecursiveShapeClassifier(model)

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

    /**
     * Renders the schema statics, the `SCHEMA` const, and the
     * `SerializableStruct` impl — no deserialization methods.
     */
    fun renderSerializeOnly() {
        val symbol = symbolProvider.toSymbol(shape)
        val codegenScope =
            arrayOf(
                "Schema" to smithySchema.resolve("Schema"),
            )
        val schemaPrefix = this.schemaPrefix ?: symbol.name.uppercase()

        renderMemberSchemas(writer, schemaPrefix)
        renderSchemaStatic(writer, schemaPrefix, symbol.name)

        // Fully-qualified impl targets: schema code renders into the dedicated
        // `schema_serde` module, not the shape's own module.
        writer.rustTemplate(
            """
            impl ${symbol.fullName} {
                /// The schema for this shape.
                pub const SCHEMA: &'static #{Schema}<'static> = &${schemaPrefix}_SCHEMA;
            }
            """,
            *codegenScope,
        )

        if (shape is StructureShape) {
            renderSerializableStruct(writer, symbol.fullName, schemaPrefix)
        } else if (shape is UnionShape) {
            renderSerializableUnion(writer, symbol.fullName, schemaPrefix)
        }
    }

    /**
     * True when a string-targeting member's resolved Rust type is not `String` —
     * i.e. a server constrained-string newtype (`publicConstrainedTypes=true`),
     * which exposes the inner `&str` via `as_str()`.
     */
    private fun isConstrainedStringMember(member: MemberShape): Boolean {
        var type = symbolProvider.toSymbol(member).rustType().stripOuter<RustType.Option>()
        if (type is RustType.Box) {
            type = type.member
        }
        return type != RustType.String
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
                    val writeCall = writeMethodForShape(target, memberSchemaRef, member)
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
                        val writeExpr = unionVariantWriteExpr(target, memberSchemaRef, "val", member)
                        rust("Self::$variantName(val) => { $writeExpr },")
                    }
                }
                // Server unions are generated without the `Unknown` variant
                // (`renderUnknownVariant = false`), so no arm is emitted for it.
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
        member: MemberShape? = null,
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
                if (isStringEnum(target) || (member != null && isConstrainedStringMember(member))) {
                    "ser.write_string(&$memberSchemaRef, $varName.as_str())?;"
                } else {
                    "ser.write_string(&$memberSchemaRef, $varName)?;"
                }
            is BlobShape -> "ser.write_blob(&$memberSchemaRef, $varName.as_ref())?;"
            is TimestampShape -> "ser.write_timestamp(&$memberSchemaRef, $varName)?;"
            is StructureShape -> "ser.write_struct(&$memberSchemaRef, $varName)?;"
            is ListShape -> {
                val elementTarget = model.expectShape(target.member.target)
                val isSparse = target.hasTrait(SparseTrait::class.java)
                // Specialized helpers (write_*_list) take `&[T]`, but sparse
                // lists generate as `&[Option<T>]`, so we can only use them
                // for non-sparse lists. Sparse lists fall through to the
                // generic write_list path below, which destructures
                // `Option<T>` per element and emits write_null for None.
                val helperExpr =
                    if (isSparse) {
                        null
                    } else {
                        when (elementTarget) {
                            is StringShape -> if (!isStringEnum(elementTarget)) "ser.write_string_list(&$memberSchemaRef, $varName)?;" else null
                            is BlobShape -> "ser.write_blob_list(&$memberSchemaRef, $varName)?;"
                            is IntegerShape, is IntEnumShape -> "ser.write_integer_list(&$memberSchemaRef, $varName)?;"
                            is LongShape -> "ser.write_long_list(&$memberSchemaRef, $varName)?;"
                            else -> null
                        }
                    }
                helperExpr ?: run {
                    val elementWrite = elementWriteExpr(target, memberSchemaRef, elementTarget, "item")
                    if (isSparse) {
                        """
                        ser.write_list(&$memberSchemaRef, &|ser: &mut dyn ::aws_smithy_schema::serde::ShapeSerializer| {
                            for item in $varName {
                                match item {
                                    Some(item) => { $elementWrite }
                                    None => { ser.write_null(&::aws_smithy_schema::prelude::STRING)?; }
                                }
                            }
                            Ok(())
                        })?;
                        """
                    } else {
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
            }
            is MapShape -> {
                val keyTarget = model.expectShape(target.key.target)
                val valueTarget = model.expectShape(target.value.target)
                val isSparse = target.hasTrait(SparseTrait::class.java)
                // The string-string map helper takes `&HashMap<String, String>`.
                // Sparse maps have `Option<String>` values, so the helper
                // doesn't apply.
                if (!isSparse && !isStringEnum(keyTarget) && valueTarget is StringShape && !isStringEnum(valueTarget)) {
                    "ser.write_string_string_map(&$memberSchemaRef, $varName)?;"
                } else {
                    val keyExpr = if (isStringEnum(keyTarget)) "key.as_str()" else "key"
                    val valueWrite = mapValueWriteExpr(target, memberSchemaRef, valueTarget, "value")
                    if (isSparse) {
                        """
                        ser.write_map(&$memberSchemaRef, &|ser: &mut dyn ::aws_smithy_schema::serde::ShapeSerializer| {
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
            }
            is UnionShape -> "ser.write_struct(&$memberSchemaRef, $varName)?;"
            is DocumentShape -> "ser.write_document(&$memberSchemaRef, $varName)?;"
            else -> "todo!(\"schema: unsupported union variant type\");"
        }
    }

    private fun writeMethodForShape(
        target: Shape,
        memberSchemaRef: String,
        member: software.amazon.smithy.model.shapes.MemberShape? = null,
    ): String {
        // For @httpPayload struct/union members, pass the target's own SCHEMA so
        // codecs use its proper name (with @xmlName, etc.) instead of the member
        // schema's member_name. JSON output is unchanged (no member_name → no
        // field-key prefix); XML now emits the correct root element name.
        val isHttpPayload =
            member?.hasTrait(software.amazon.smithy.model.traits.HttpPayloadTrait::class.java) == true
        val structSchemaRef =
            if (isHttpPayload) {
                "${symbolProvider.toSymbol(target).fullName}::SCHEMA"
            } else {
                "&$memberSchemaRef"
            }
        return when (target) {
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
                if (isStringEnum(target) || (member != null && isConstrainedStringMember(member))) {
                    // Constrained-string newtypes expose the inner `&str` via `as_str()`.
                    "ser.write_string(&$memberSchemaRef, val.as_str())?;"
                } else {
                    "ser.write_string(&$memberSchemaRef, val)?;"
                }

            is BlobShape ->
                if (target.hasTrait(StreamingTrait::class.java)) {
                    "// streaming blob is serialized as the HTTP body by the protocol, not the codec"
                } else {
                    "ser.write_blob(&$memberSchemaRef, val.as_ref())?;"
                }

            is TimestampShape -> "ser.write_timestamp(&$memberSchemaRef, val)?;"
            is DocumentShape -> "ser.write_document(&$memberSchemaRef, val)?;"
            is ListShape -> {
                val isSparse = target.hasTrait(SparseTrait::class.java)
                val elementTarget = model.expectShape(target.member.target)
                val elementWrite = elementWriteExpr(target, memberSchemaRef, elementTarget, "item")
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
                val valueWrite = mapValueWriteExpr(target, memberSchemaRef, valueTarget, "value")
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

            is StructureShape -> "ser.write_struct($structSchemaRef, val)?;"
            is UnionShape -> "ser.write_struct($structSchemaRef, val)?;"
            else -> "todo!(\"schema: unsupported shape type for serialization\");"
        }
    }

    /**
     * Returns a write expression for a list element (no member name needed).
     *
     * [containingAggregate] is the list whose elements we're writing.
     * [parentRef] is the Rust schema constant name for that containing list,
     * used to derive the inner element's schema constant
     * (`<parent>_MEMBER`) when the element is itself a nested aggregate.
     * `null` means we're past a recursive boundary upstream — every nested
     * aggregate from here down falls back to `prelude::DOCUMENT`.
     */
    private fun elementWriteExpr(
        containingAggregate: Shape,
        parentRef: String?,
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

            is BlobShape -> "ser.write_blob(&$prelude::BLOB, $varName.as_ref())?;"
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
                val isSparse = target.hasTrait(SparseTrait::class.java)
                // We're writing a list element that is itself a map. The map's
                // schema at this position is the containing list's `_MEMBER`
                // chain — unless we're in placeholder mode upstream
                // (parentRef == null) or this target closes a cycle back to
                // the containing list.
                val nextRef =
                    if (parentRef != null && !recursiveClassifier.isRecursive(containingAggregate, target)) {
                        "${parentRef}_MEMBER"
                    } else {
                        null
                    }
                val schemaExpr = nextRef?.let { "&$it" } ?: "&::aws_smithy_schema::prelude::DOCUMENT"
                val valueWrite = mapValueWriteExpr(target, nextRef, valueTarget, "value")
                if (isSparse) {
                    """
                    ser.write_map($schemaExpr, &|ser: &mut dyn ::aws_smithy_schema::serde::ShapeSerializer| {
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
                    ser.write_map($schemaExpr, &|ser: &mut dyn ::aws_smithy_schema::serde::ShapeSerializer| {
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
                val isSparse = target.hasTrait(SparseTrait::class.java)
                val nextRef =
                    if (parentRef != null && !recursiveClassifier.isRecursive(containingAggregate, target)) {
                        "${parentRef}_MEMBER"
                    } else {
                        null
                    }
                val schemaExpr = nextRef?.let { "&$it" } ?: "&::aws_smithy_schema::prelude::DOCUMENT"
                val elementWrite = elementWriteExpr(target, nextRef, elementTarget, "item")
                if (isSparse) {
                    """
                    ser.write_list($schemaExpr, &|ser: &mut dyn ::aws_smithy_schema::serde::ShapeSerializer| {
                        for item in $varName {
                            match item {
                                Some(item) => { $elementWrite }
                                None => { ser.write_null(&::aws_smithy_schema::prelude::STRING)?; }
                            }
                        }
                        Ok(())
                    })?;
                    """
                } else {
                    """
                    ser.write_list($schemaExpr, &|ser: &mut dyn ::aws_smithy_schema::serde::ShapeSerializer| {
                        for item in $varName {
                            $elementWrite
                        }
                        Ok(())
                    })?;
                    """
                }
            }

            is UnionShape -> {
                val targetQualified = symbolProvider.toSymbol(target).rustType().qualifiedName()
                "ser.write_struct($targetQualified::SCHEMA, $varName)?;"
            }

            else -> "todo!(\"schema: unsupported list element type\");"
        }
    }

    /**
     * Returns a write expression for a map value.
     *
     * [containingAggregate] is the map whose values we're writing.
     * [parentRef] is the Rust schema constant name for that containing map,
     * used to derive the inner value's schema constant (`<parent>_VALUE`)
     * when the value is itself a nested aggregate. `null` means we're past
     * a recursive boundary upstream — every nested aggregate from here down
     * falls back to `prelude::DOCUMENT`.
     */
    private fun mapValueWriteExpr(
        containingAggregate: Shape,
        parentRef: String?,
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

            is BlobShape -> "ser.write_blob(&$prelude::BLOB, $varName.as_ref())?;"
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
                val isSparse = target.hasTrait(SparseTrait::class.java)
                // We're writing a map value that is itself a map. Its schema
                // at this position is the containing map's `_VALUE` chain —
                // unless we're already in placeholder mode or this target
                // closes a cycle back to the containing map.
                val nextRef =
                    if (parentRef != null && !recursiveClassifier.isRecursive(containingAggregate, target)) {
                        "${parentRef}_VALUE"
                    } else {
                        null
                    }
                val schemaExpr = nextRef?.let { "&$it" } ?: "&$prelude::DOCUMENT"
                val innerValueWrite = mapValueWriteExpr(target, nextRef, valueTarget, "value")
                if (isSparse) {
                    """
                    ser.write_map($schemaExpr, &|ser: &mut dyn ::aws_smithy_schema::serde::ShapeSerializer| {
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
                    ser.write_map($schemaExpr, &|ser: &mut dyn ::aws_smithy_schema::serde::ShapeSerializer| {
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
                val isSparse = target.hasTrait(SparseTrait::class.java)
                val nextRef =
                    if (parentRef != null && !recursiveClassifier.isRecursive(containingAggregate, target)) {
                        "${parentRef}_VALUE"
                    } else {
                        null
                    }
                val schemaExpr = nextRef?.let { "&$it" } ?: "&$prelude::DOCUMENT"
                val elementWrite = elementWriteExpr(target, nextRef, elementTarget, "item")
                if (isSparse) {
                    """
                    ser.write_list($schemaExpr, &|ser: &mut dyn ::aws_smithy_schema::serde::ShapeSerializer| {
                        for item in $varName {
                            match item {
                                Some(item) => { $elementWrite }
                                None => { ser.write_null(&$prelude::STRING)?; }
                            }
                        }
                        Ok(())
                    })?;
                    """
                } else {
                    """
                    ser.write_list($schemaExpr, &|ser: &mut dyn ::aws_smithy_schema::serde::ShapeSerializer| {
                        for item in $varName {
                            $elementWrite
                        }
                        Ok(())
                    })?;
                    """
                }
            }

            is UnionShape -> {
                val targetQualified = symbolProvider.toSymbol(target).rustType().qualifiedName()
                "ser.write_struct($targetQualified::SCHEMA, $varName)?;"
            }

            else -> "todo!(\"schema: unsupported map value type\");"
        }
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
                        """map.insert(Box::new(#{AnnotationTrait}::new(#{ShapeId}::from_parts("$traitNs##$traitName", "$traitNs", "$traitName"))));""",
                        *codegenScope,
                    )
                } else if (stringValue != null) {
                    rustTemplate(
                        """map.insert(Box::new(#{StringTrait}::new(#{ShapeId}::from_parts("$traitNs##$traitName", "$traitNs", "$traitName"), ${stringValue.dq()})));""",
                        *codegenScope,
                    )
                } else {
                    // Render the trait's structured value as a structured `Document`
                    // (object/array/number/bool/string), preserving the shape of the
                    // value instead of flattening it to a JSON string. The runtime
                    // `Document` type can represent the full Smithy data model, so an
                    // unknown trait's value round-trips structurally (per the SEP:
                    // unknown trait values "should be represented with a document data
                    // type").
                    rustTemplate(
                        """map.insert(Box::new(#{DocumentTrait}::new(#{ShapeId}::from_parts("$traitNs##$traitName", "$traitNs", "$traitName"), #{docValue})));""",
                        *codegenScope,
                        "docValue" to nodeToDocument(trait.toNode()),
                    )
                }
            }
        }

    /**
     * Renders a Smithy trait value [Node] as a [Writable] that constructs the
     * structurally-equivalent [`aws_smithy_types::Document`].
     *
     * Used for unknown traits whose value is not a plain string, so the generated
     * schema preserves the trait's structure (nested objects, arrays, numbers,
     * booleans) rather than flattening it to a single JSON string.
     *
     * Uses [RuntimeType] symbols (not hardcoded paths) so the `aws-smithy-types`
     * dependency is registered on the generated crate. `#` inside string literals
     * is escaped as `##` so the result is safe inside a `rustTemplate`.
     */
    private fun nodeToDocument(node: Node): Writable =
        writable {
            val docScope =
                arrayOf(
                    "Document" to RuntimeType.smithyTypes(runtimeConfig).resolve("Document"),
                    "Number" to RuntimeType.smithyTypes(runtimeConfig).resolve("Number"),
                    "DocumentObject" to RuntimeType.smithyTypes(runtimeConfig).resolve("document::DocumentObject"),
                )

            fun escape(s: String) = s.replace("\\", "\\\\").replace("\"", "\\\"").replace("#", "##")
            when {
                node.isNullNode -> rustTemplate("#{Document}::Null", *docScope)
                node.isBooleanNode -> rustTemplate("#{Document}::Bool(${node.expectBooleanNode().value})", *docScope)
                node.isStringNode ->
                    rustTemplate("""#{Document}::String("${escape(node.expectStringNode().value)}".to_string())""", *docScope)
                node.isNumberNode -> {
                    val number = node.expectNumberNode()
                    if (number.isFloatingPointNumber) {
                        rustTemplate("#{Document}::Number(#{Number}::Float(${number.value.toDouble()}f64))", *docScope)
                    } else {
                        val value = number.value.toLong()
                        if (value >= 0) {
                            rustTemplate("#{Document}::Number(#{Number}::PosInt(${value}u64))", *docScope)
                        } else {
                            rustTemplate("#{Document}::Number(#{Number}::NegInt(${value}i64))", *docScope)
                        }
                    }
                }
                node.isArrayNode -> {
                    rustTemplate("#{Document}::Array(vec![", *docScope)
                    node.expectArrayNode().elements.forEach { element ->
                        nodeToDocument(element)(this)
                        rust(", ")
                    }
                    rust("])")
                }
                node.isObjectNode -> {
                    rustTemplate("{ let mut obj = #{DocumentObject}::new(); ", *docScope)
                    node.expectObjectNode().stringMap.entries.forEach { (key, value) ->
                        rust("""obj.insert("${escape(key)}".to_string(), """)
                        nodeToDocument(value)(this)
                        rust("); ")
                    }
                    rustTemplate("#{Document}::Object(obj) }", *docScope)
                }
                // Node is sealed over the cases above; this is unreachable for valid models.
                else -> rustTemplate("#{Document}::Null", *docScope)
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
     * Returns the full trait setter chain to append after a member schema's
     * `Schema::new_member(...)` constructor:
     *
     *  - `traitSetterChain(member)`: all known serde traits set directly on
     *    the member shape (e.g., `@xmlName`, `@httpHeader`).
     *  - `@timestampFormat` propagated from the target shape when the member
     *    doesn't carry it itself and the target is a timestamp.
     *  - `@mediaType` propagated from the target shape when the member doesn't
     *    carry it itself.
     *
     * Used for struct/union members, list members, map keys, and map values —
     * any [MemberShape] that gets emitted as a `_MEMBER` / `_KEY` / `_VALUE`
     * schema constant. Mirrors Smithy semantics that target-shape traits apply
     * transitively unless overridden by the member.
     */
    private fun memberTraitChain(member: software.amazon.smithy.model.shapes.MemberShape): String {
        val target = model.expectShape(member.target)
        val baseChain = traitSetterChain(member)
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
        val targetMediaType =
            if (
                !member.hasTrait(software.amazon.smithy.model.traits.MediaTypeTrait::class.java) &&
                target.hasTrait(software.amazon.smithy.model.traits.MediaTypeTrait::class.java)
            ) {
                knownTraitSetter(
                    target.expectTrait(software.amazon.smithy.model.traits.MediaTypeTrait::class.java),
                ) ?: ""
            } else {
                ""
            }
        return baseChain + targetTimestampFormat + targetMediaType
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

    /**
     * If this shape is the output of an operation carrying the AWS
     * S3 `S3UnwrappedXmlOutputTrait` customization, returns a
     * `.with_xml_unwrapped_output()` chain.
     *
     * The trait is operation-level but its effect (the XML wire format
     * omits the outer wrapper element) only matters for the OUTPUT
     * struct's deserialization, so we surface it on the output schema.
     * The XML codec reads `schema.xml_unwrapped_output()` when
     * deserializing; other codecs ignore it. Schema-level metadata
     * (rather than codegen-level body wrapping) keeps runtime protocol
     * swap unaffected.
     */
    private fun s3UnwrappedXmlOutputChain(shape: Shape): String {
        val operationIndex = software.amazon.smithy.model.knowledge.OperationIndex.of(model)
        for (operation in model.operationShapes) {
            if (operationIndex.getOutputShape(operation).orElse(null)?.id == shape.id &&
                operation.hasTrait(software.amazon.smithy.aws.traits.customizations.S3UnwrappedXmlOutputTrait::class.java)
            ) {
                return "\n    .with_xml_unwrapped_output()"
            }
        }
        return ""
    }

    /**
     * If this shape is the input of any operation AND every member is
     * HTTP-bound (i.e., carries one of `@httpHeader`, `@httpQuery`,
     * `@httpLabel`, `@httpPrefixHeaders`, `@httpQueryParams`, or scalar
     * `@httpPayload`) — equivalently, no member serializes to the request
     * body — returns `.with_no_body_members()`.
     *
     * The runtime uses this signal to skip body-codec invocation entirely
     * on the SER path: no XML/JSON wrapper element is opened, no
     * `serialize_members` re-entry through the codec proxy fires, and the
     * empty body bytes are never collected. Saves ~15-20% on header-only
     * SER operations like S3 PutObject / CopyObject.
     *
     * The semantics intentionally mirror the runtime's existing inline
     * `has_body_members` computation in
     * `HttpBindingProtocol::serialize_request_with_body` so that
     * codegen-set `with_no_body_members()` always agrees with the runtime
     * check that gates `Content-Type` / empty-body handling.
     *
     * `@httpPayload` on a struct/union counts as a body member because it
     * provides body framing through the codec; `@httpPayload` on a blob
     * or string does NOT (the bytes go directly into the request body
     * without ever touching the codec).
     */
    private fun noBodyMembersChain(shape: Shape): String {
        val operationIndex = software.amazon.smithy.model.knowledge.OperationIndex.of(model)
        val isOperationInput =
            model.operationShapes.any {
                operationIndex.getInputShape(it).orElse(null)?.id == shape.id
            }
        if (!isOperationInput) return ""
        if (shape !is software.amazon.smithy.model.shapes.StructureShape) return ""

        for (member in shape.allMembers.values) {
            val hasHttpHeader = member.hasTrait(software.amazon.smithy.model.traits.HttpHeaderTrait::class.java)
            val hasHttpQuery = member.hasTrait(software.amazon.smithy.model.traits.HttpQueryTrait::class.java)
            val hasHttpLabel = member.hasTrait(software.amazon.smithy.model.traits.HttpLabelTrait::class.java)
            val hasHttpPrefixHeaders = member.hasTrait(software.amazon.smithy.model.traits.HttpPrefixHeadersTrait::class.java)
            val hasHttpQueryParams = member.hasTrait(software.amazon.smithy.model.traits.HttpQueryParamsTrait::class.java)
            val hasHttpPayload = member.hasTrait(software.amazon.smithy.model.traits.HttpPayloadTrait::class.java)

            // Member without ANY HTTP binding → goes to body. Schema has body members.
            if (!hasHttpHeader && !hasHttpQuery && !hasHttpLabel &&
                !hasHttpPrefixHeaders && !hasHttpQueryParams && !hasHttpPayload
            ) {
                return ""
            }
            // `@httpPayload` on a struct/union → body framing comes from the
            // codec writing the payload member's wrapper element. Counts as
            // a body member from the runtime's perspective.
            if (hasHttpPayload) {
                val target = model.expectShape(member.target)
                if (target is software.amazon.smithy.model.shapes.StructureShape ||
                    target is software.amazon.smithy.model.shapes.UnionShape
                ) {
                    return ""
                }
            }
        }
        return "\n    .with_no_body_members()"
    }

    /**
     * If this shape carries `SyntheticInputTrait` or `SyntheticOutputTrait`
     * with a non-null `originalId`, returns a `.with_original_name(...)` call
     * that surfaces the original (pre-synthesis) shape name. REST XML reads
     * this when constructing the body root element name; other consumers
     * (logging, future protocols) may also read it. Returns "" otherwise.
     */
    private fun originalNameChain(shape: Shape): String {
        val originalName =
            shape.getTrait(SyntheticInputTrait::class.java).orElse(null)?.originalId?.name
                ?: shape.getTrait(SyntheticOutputTrait::class.java).orElse(null)?.originalId?.name
                ?: return ""
        return "\n    .with_original_name(${originalName.dq()})"
    }

    /**
     * For a member targeting a list or map, emits the corresponding nested
     * member sub-schema statics (`_KEY` / `_VALUE` for map, `_MEMBER` for
     * list) and returns a `.with_map_members(...)` / `.with_list_member(...)`
     * chain string to attach to the parent member's schema. Recurses for
     * nested aggregates (e.g., `map<string, map<...>>`, `list<list<...>>`)
     * so the entire aggregate sub-graph is reachable from the runtime via
     * `Schema::key()` / `.value()` / `.member()`.
     *
     * Returns `""` for non-aggregate targets.
     *
     * Termination invariant: this recursion descends only through aggregate
     * members (list element, map key/value) and stops at structure/union targets
     * (the `else -> ""` arm) and scalars. A structure/union carries its own
     * top-level `::SCHEMA` constant, so the descent never crosses that boundary.
     * Combined with the Smithy guarantee that a recursive list/map/set is valid
     * only if its cycle passes through a structure or union, the descent is
     * bounded for any valid model. The same boundary bounds the sibling write-expr
     * recursion ([elementWriteExpr] / [mapValueWriteExpr]). A hand-built model that
     * violated the invariant (an aggregate-only cycle) would not terminate, but
     * such models are rejected by Smithy validation before reaching codegen.
     */
    private fun emitAggregateMemberChain(
        writer: RustWriter,
        prefix: String,
        target: Shape,
        codegenScope: Array<out Pair<String, Any>>,
    ): String =
        when (target) {
            is MapShape -> {
                val keyTarget = model.expectShape(target.key.target)
                val valueTarget = model.expectShape(target.value.target)
                val escapedKeyId = target.key.id.toString().replace("#", "##")
                val escapedValueId = target.value.id.toString().replace("#", "##")
                val keyTraitChain = memberTraitChain(target.key)
                val valueTraitChain = memberTraitChain(target.value)
                // Recurse before emitting so nested chains attach correctly.
                val keyAggChain = emitAggregateMemberChain(writer, "${prefix}_KEY", keyTarget, codegenScope)
                val valueAggChain = emitAggregateMemberChain(writer, "${prefix}_VALUE", valueTarget, codegenScope)
                writer.rustTemplate(
                    """
                    static ${prefix}_KEY: #{Schema}<'static> = #{Schema}::new_member(
                        #{ShapeId}::from_parts(
                            "$escapedKeyId",
                            "${target.key.id.namespace}",
                            "${target.key.id.name}",
                        ),
                        #{ShapeType}::${shapeTypeVariant(keyTarget)},
                        "key",
                        0,
                    )$keyTraitChain$keyAggChain;
                    static ${prefix}_VALUE: #{Schema}<'static> = #{Schema}::new_member(
                        #{ShapeId}::from_parts(
                            "$escapedValueId",
                            "${target.value.id.namespace}",
                            "${target.value.id.name}",
                        ),
                        #{ShapeType}::${shapeTypeVariant(valueTarget)},
                        "value",
                        1,
                    )$valueTraitChain$valueAggChain;
                    """,
                    *codegenScope,
                )
                "\n    .with_map_members(&${prefix}_KEY, &${prefix}_VALUE)"
            }
            is ListShape -> {
                val listMemberTarget = model.expectShape(target.member.target)
                val escapedListMemberId = target.member.id.toString().replace("#", "##")
                val listMemberTraitChain = memberTraitChain(target.member)
                val nestedChain =
                    emitAggregateMemberChain(writer, "${prefix}_MEMBER", listMemberTarget, codegenScope)
                writer.rustTemplate(
                    """
                    static ${prefix}_MEMBER: #{Schema}<'static> = #{Schema}::new_member(
                        #{ShapeId}::from_parts(
                            "$escapedListMemberId",
                            "${target.member.id.namespace}",
                            "${target.member.id.name}",
                        ),
                        #{ShapeType}::${shapeTypeVariant(listMemberTarget)},
                        "member",
                        0,
                    )$listMemberTraitChain$nestedChain;
                    """,
                    *codegenScope,
                )
                "\n    .with_list_member(&${prefix}_MEMBER)"
            }
            else -> ""
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
            "smithy.api#xmlNamespace" -> {
                val ns = trait as XmlNamespaceTrait
                val prefix = ns.prefix.map { "Some(${it.dq()})" }.orElse("None")
                "\n    .with_xml_namespace(${ns.uri.dq()}, $prefix)"
            }
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
                "ShapeId" to smithySchema.resolve("ShapeId"),
                "ShapeType" to smithySchema.resolve("ShapeType"),
            )

        // The shape ID is constructed inline in the `Schema::new_*` call below
        // rather than referencing a separate `static ..._SCHEMA_ID`. `ShapeId`
        // is not `Copy`, so moving it out of a `static` into another `static`
        // initializer would fail (a `const` context cannot call `.clone()`).
        // Constructing it inline moves a `const` temporary into the `const fn`
        // constructor, which is allowed — the same pattern member schemas use.
        val ns = shape.id.namespace
        val name = shape.id.name
        val escapedFqn = shape.id.toString().replace("#", "##")
        val schemaIdExpr = """#{ShapeId}::from_parts("$escapedFqn", "$ns", "$name")"""

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
                val traitChain =
                    traitSetterChain(shape) + httpTraitChain(shape) +
                        s3UnwrappedXmlOutputChain(shape) + noBodyMembersChain(shape) +
                        originalNameChain(shape)
                if (hasUnknownTraits(shape)) {
                    writer.rustTemplate(
                        """
                        static ${schemaPrefix}_TRAITS: std::sync::LazyLock<#{TraitMap}> = std::sync::LazyLock::new(|| {
                            let mut map = #{TraitMap}::new();
                            #{insertions}
                            map
                        });
                        static ${schemaPrefix}_SCHEMA: #{Schema}<'static> = #{Schema}::new_struct(
                            $schemaIdExpr,
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
                        static ${schemaPrefix}_SCHEMA: #{Schema}<'static> = #{Schema}::new_struct(
                            $schemaIdExpr,
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
                    static ${schemaPrefix}_SCHEMA: #{Schema}<'static> = #{Schema}::new_list(
                        $schemaIdExpr,
                        &${schemaPrefix}_MEMBER,
                    );
                    """,
                    *codegenScope,
                )
            }

            is MapShape -> {
                writer.rustTemplate(
                    """
                    static ${schemaPrefix}_SCHEMA: #{Schema}<'static> = #{Schema}::new_map(
                        $schemaIdExpr,
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
                    static ${schemaPrefix}_SCHEMA: #{Schema}<'static> = #{Schema}::new(
                        $schemaIdExpr,
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
                    val traitChain = memberTraitChain(member)
                    val memberConstName = "${schemaPrefix}_MEMBER_${constantName(rustMemberName)}"

                    // For map / list members, emit key/value/element sub-schemas so the XML
                    // codec can resolve entry element names. Recurses through nested
                    // list/map shapes so the entire aggregate sub-graph is reachable from
                    // the runtime via Schema::key() / .value() / .member().
                    val mapMembersChain =
                        emitAggregateMemberChain(writer, memberConstName, target, codegenScope)

                    writer.rustTemplate(
                        """
                        static $memberConstName: #{Schema}<'static> = #{Schema}::new_member(
                            #{ShapeId}::from_parts(
                                "$escapedMemberId",
                                "${member.id.namespace}",
                                "${member.id.name}",
                            ),
                            #{ShapeType}::${shapeTypeVariant(target)},
                            ${templateEscape(smithyMemberName.dq())},
                            $idx,
                        )$traitChain$mapMembersChain;
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
                        static ${schemaPrefix}_MEMBER_${constantName(synth.fieldName)}: #{Schema}<'static> = #{Schema}::new_member(
                            #{ShapeId}::from_parts(
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
                val traitChain = memberTraitChain(shape.member)
                writer.rustTemplate(
                    """
                    static ${schemaPrefix}_MEMBER: #{Schema}<'static> = #{Schema}::new_member(
                        #{ShapeId}::from_parts(
                            "$escapedMemberId",
                            "${shape.member.id.namespace}",
                            "${shape.member.id.name}",
                        ),
                        #{ShapeType}::${shapeTypeVariant(target)},
                        "member",
                        0,
                    )$traitChain;
                    """,
                    *codegenScope,
                )
            }

            is MapShape -> {
                val keyTarget = model.expectShape(shape.key.target)
                val valueTarget = model.expectShape(shape.value.target)
                val escapedKeyId = shape.key.id.toString().replace("#", "##")
                val escapedValueId = shape.value.id.toString().replace("#", "##")
                val keyTraitChain = memberTraitChain(shape.key)
                val valueTraitChain = memberTraitChain(shape.value)
                writer.rustTemplate(
                    """
                    static ${schemaPrefix}_KEY: #{Schema}<'static> = #{Schema}::new_member(
                        #{ShapeId}::from_parts(
                            "$escapedKeyId",
                            "${shape.key.id.namespace}",
                            "${shape.key.id.name}",
                        ),
                        #{ShapeType}::${shapeTypeVariant(keyTarget)},
                        "key",
                        0,
                    )$keyTraitChain;

                    static ${schemaPrefix}_VALUE: #{Schema}<'static> = #{Schema}::new_member(
                        #{ShapeId}::from_parts(
                            "$escapedValueId",
                            "${shape.value.id.namespace}",
                            "${shape.value.id.name}",
                        ),
                        #{ShapeType}::${shapeTypeVariant(valueTarget)},
                        "value",
                        1,
                    )$valueTraitChain;
                    """,
                    *codegenScope,
                )
            }
        }
    }
}