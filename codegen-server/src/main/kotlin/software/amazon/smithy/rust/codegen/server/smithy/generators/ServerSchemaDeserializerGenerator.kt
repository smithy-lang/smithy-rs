/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

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
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShortShape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.TimestampShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.model.traits.SparseTrait
import software.amazon.smithy.model.traits.StreamingTrait
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.qualifiedName
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.isOptional
import software.amazon.smithy.rust.codegen.core.smithy.isRustBoxed
import software.amazon.smithy.rust.codegen.core.smithy.rustType
import software.amazon.smithy.rust.codegen.core.smithy.protocols.shapeModuleName
import software.amazon.smithy.rust.codegen.core.util.isTargetUnit
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.canReachConstrainedShape

/**
 * Generates the server-side schema-driven deserialization walker for a structure or
 * union shape (plan 2g, Design B): a `pub(crate) fn deser_<shape>` in the shape's
 * `schema_serde` module that drives a [`ShapeDeserializer`] into the shape's *parse
 * symbol* — the same type the legacy protocol parsers produced:
 *
 * - structure that can reach a constrained shape → its internal (unconstrained)
 *   builder; the single top-level `build()` on the operation input enforces every
 *   constraint (`@required`/`@length`/`@range`/`@pattern`/`@enum`), producing
 *   today's `ConstraintViolation` values with frozen messages (principle 3).
 * - structure that cannot → the structure itself (its builder is infallible).
 * - union that can reach a constrained shape → its `XxxUnconstrained` mirror;
 *   otherwise the union itself.
 * - aggregates (lists/maps) are read inline with runtime element-schema lookups,
 *   wrapped in their unconstrained tuple structs where the aggregate can reach a
 *   constrained shape.
 *
 * Server semantics — deliberately NOT the client's `deserialize()` pattern:
 * no error correction, no defaulting, no `Unknown` union arms (an unknown union
 * variant is a wire-level error → protocol 4xx).
 *
 * For operation input shapes, [renderDeserializableShapeImpl] additionally
 * implements the runtime seam `DeserializableShape`, the target of
 * `ServerProtocol::deserialize_request`.
 */
class ServerSchemaDeserializerGenerator(
    private val codegenContext: ServerCodegenContext,
    private val writer: RustWriter,
    private val shape: Shape,
) {
    private val model = codegenContext.model
    private val symbolProvider = codegenContext.symbolProvider
    private val unconstrainedShapeSymbolProvider = codegenContext.unconstrainedShapeSymbolProvider
    private val serviceShape = codegenContext.serviceShape
    private val runtimeConfig = codegenContext.runtimeConfig

    private fun canReachConstrained(shape: Shape): Boolean =
        shape.canReachConstrainedShape(model, symbolProvider)

    /** The type the deser fn for [shape] produces — mirrors the legacy `returnSymbolToParseFn`. */
    private fun parseSymbolFullName(shape: Shape): String =
        if (canReachConstrained(shape)) {
            unconstrainedShapeSymbolProvider.toSymbol(shape).rustType().qualifiedName()
        } else {
            symbolProvider.toSymbol(shape).rustType().qualifiedName()
        }

    private fun deserFnName(shape: Shape): String = "deser_" + symbolProvider.toSymbol(shape).name.toSnakeCase()

    /** Fully-qualified path of the deser fn for a struct/union [target] in its own schema_serde module. */
    private fun deserFnPath(target: Shape): String =
        "crate::schema_serde::${symbolProvider.shapeModuleName(serviceShape, target)}::${deserFnName(target)}"

    /** Escapes `#` (raw identifiers like `r#type`) for embedding in RustWriter format strings. */
    private fun esc(s: String): String = s.replace("#", "##")

    private fun isStringEnum(shape: Shape): Boolean = shape is EnumShape || shape.hasTrait(EnumTrait::class.java)

    private fun preludeConstFor(shape: Shape): String {
        val prelude = "::aws_smithy_schema::prelude"
        return when (shape) {
            is BooleanShape -> "$prelude::BOOLEAN"
            is ByteShape -> "$prelude::BYTE"
            is ShortShape -> "$prelude::SHORT"
            is IntEnumShape -> "$prelude::INTEGER"
            is IntegerShape -> "$prelude::INTEGER"
            is LongShape -> "$prelude::LONG"
            is FloatShape -> "$prelude::FLOAT"
            is DoubleShape -> "$prelude::DOUBLE"
            is BigIntegerShape -> "$prelude::BIG_INTEGER"
            is BigDecimalShape -> "$prelude::BIG_DECIMAL"
            is StringShape -> "$prelude::STRING"
            is BlobShape -> "$prelude::BLOB"
            is TimestampShape -> "$prelude::TIMESTAMP"
            else -> "$prelude::DOCUMENT"
        }
    }

    /**
     * Returns a Rust expression reading a value of [target]'s parse type from `deser`,
     * using [schemaExpr] (an expression of type `&Schema<'_>`) as the value's schema.
     *
     * Strings and enums both read as plain `String` — enums are constrained shapes
     * server-side; conversion happens at `build()` (frozen `ConstraintViolation`s).
     */
    private fun readTargetExpr(
        target: Shape,
        schemaExpr: String,
    ): String =
        when (target) {
            is BooleanShape -> "deser.read_boolean($schemaExpr)?"
            is ByteShape -> "deser.read_byte($schemaExpr)?"
            is ShortShape -> "deser.read_short($schemaExpr)?"
            is IntEnumShape -> "deser.read_integer($schemaExpr)?"
            is IntegerShape -> "deser.read_integer($schemaExpr)?"
            is LongShape -> "deser.read_long($schemaExpr)?"
            is FloatShape -> "deser.read_float($schemaExpr)?"
            is DoubleShape -> "deser.read_double($schemaExpr)?"
            is BigIntegerShape -> "deser.read_big_integer($schemaExpr)?"
            is BigDecimalShape -> "deser.read_big_decimal($schemaExpr)?"
            is EnumShape -> "deser.read_string($schemaExpr)?"
            is StringShape -> "deser.read_string($schemaExpr)?"
            is BlobShape -> "deser.read_blob($schemaExpr)?"
            is TimestampShape -> "deser.read_timestamp($schemaExpr)?"
            is DocumentShape -> "deser.read_document($schemaExpr)?"
            is StructureShape, is UnionShape -> "${deserFnPath(target)}(deser)?"
            is ListShape -> listReadExpr(target, schemaExpr)
            is MapShape -> mapReadExpr(target, schemaExpr)
            else -> throw IllegalArgumentException("schema deserializer: unsupported target ${target.id}")
        }

    private fun sparseAware(
        collection: Shape,
        readExpr: String,
    ): String =
        if (collection.hasTrait(SparseTrait::class.java)) {
            "if deser.is_null() { deser.read_null()?; None } else { Some($readExpr) }"
        } else {
            readExpr
        }

    private fun listReadExpr(
        list: ListShape,
        schemaExpr: String,
    ): String {
        val elementTarget = model.expectShape(list.member.target)
        val isSparse = list.hasTrait(SparseTrait::class.java)
        val canReach = canReachConstrained(list)
        // Cheap helpers for plain (fully unconstrained) simple lists — same closed
        // set the client walker uses.
        if (!canReach && !isSparse) {
            val helper =
                when (elementTarget) {
                    is StringShape -> if (!isStringEnum(elementTarget)) "read_string_list" else null
                    is BlobShape -> "read_blob_list"
                    is IntEnumShape -> null
                    is IntegerShape -> "read_integer_list"
                    is LongShape -> "read_long_list"
                    else -> null
                }
            if (helper != null) {
                return "deser.$helper($schemaExpr)?"
            }
        }
        val elementExpr = readTargetExpr(elementTarget, "element_schema")
        val pushed = sparseAware(list, elementExpr)
        val result = if (canReach) "${parseSymbolFullName(list)}(items)" else "items"
        return """{
            let list_schema = $schemaExpr;
            let element_schema = list_schema.member().unwrap_or(&${preludeConstFor(elementTarget)});
            let mut items = ::std::vec::Vec::new();
            deser.read_list(list_schema, &mut |deser| {
                items.push($pushed);
                Ok(())
            })?;
            $result
        }"""
    }

    private fun mapReadExpr(
        map: MapShape,
        schemaExpr: String,
    ): String {
        val valueTarget = model.expectShape(map.value.target)
        val keyTarget = model.expectShape(map.key.target)
        val isSparse = map.hasTrait(SparseTrait::class.java)
        val canReach = canReachConstrained(map)
        if (!canReach && !isSparse && !isStringEnum(keyTarget) &&
            valueTarget is StringShape && !isStringEnum(valueTarget)
        ) {
            return "deser.read_string_string_map($schemaExpr)?"
        }
        val valueExpr = readTargetExpr(valueTarget, "value_schema")
        val inserted = sparseAware(map, valueExpr)
        val result = if (canReach) "${parseSymbolFullName(map)}(map)" else "map"
        // Keys always read as plain `String`: constrained/enum keys stay unconstrained
        // in the parse type; key constraint enforcement happens at `build()`.
        return """{
            let map_schema = $schemaExpr;
            let value_schema = map_schema.member().unwrap_or(&${preludeConstFor(valueTarget)});
            let mut map = ::std::collections::HashMap::new();
            deser.read_map(map_schema, &mut |key, deser| {
                let value = $inserted;
                map.insert(key, value);
                Ok(())
            })?;
            $result
        }"""
    }

    /**
     * Renders the `pub(crate) fn deser_<shape>` walker for this struct/union.
     * Assumes the shape's schema statics ([ServerSchemaGenerator.renderSerializeOnly])
     * render into the same module — the walker references `<PREFIX>_SCHEMA`.
     */
    fun render() {
        when (shape) {
            is StructureShape -> renderStructDeserFn(shape)
            is UnionShape -> renderUnionDeserFn(shape)
            else -> throw IllegalArgumentException("schema deserializer: only structs/unions get deser fns, got ${shape.id}")
        }
    }

    private fun renderStructDeserFn(shape: StructureShape) {
        val symbol = symbolProvider.toSymbol(shape)
        val schemaPrefix = symbol.name.uppercase()
        val fallible = canReachConstrained(shape)
        val builderPath = shape.serverBuilderSymbol(codegenContext).rustType().qualifiedName()
        val parse = parseSymbolFullName(shape)
        val members = shape.allMembers.values.toList()

        val arms = StringBuilder()
        members.forEachIndexed { idx, member ->
            val target = model.expectShape(member.target)
            // Streaming members (event streams, streaming blobs) never come through the
            // codec walker; operations carrying them are schema-served via specialized
            // glue (plan Step 4.8) and their prelude members only.
            if (target.hasTrait(StreamingTrait::class.java)) {
                return@forEachIndexed
            }
            // Feed the builder through its `pub(crate) set_*` setters — the builder's
            // unconstrained-type ingestion surface (principle 3: the walker validates
            // nothing; the single top-level `build()` enforces all constraints,
            // producing today's `ConstraintViolation` values with frozen messages).
            val setterName = "set_" + member.memberName.toSnakeCase()
            val expr = readTargetExpr(target, "member")
            val boxed = symbolProvider.toSymbol(member).isRustBoxed()
            val bare = if (boxed) "::std::boxed::Box::new($expr.into())" else expr
            val value =
                if (symbolProvider.toSymbol(member).isOptional()) {
                    "Some($bare)"
                } else {
                    bare
                }
            arms.append(
                """
                Some($idx) => {
                    if deser.is_null() { deser.read_null()?; } else {
                        builder = ::std::mem::take(&mut builder).$setterName($value);
                    }
                }
                """,
            )
        }

        val result =
            if (fallible) {
                // Parse symbol IS the builder; the operation-level walker (or the
                // enclosing shape's `build()`) enforces constraints.
                check(parse == builderPath) {
                    "parse symbol for fallible struct ${shape.id} should be its builder ($builderPath), got $parse"
                }
                "builder"
            } else {
                "builder.build()"
            }

        writer.rust(
            """
            ##[allow(clippy::needless_question_mark)]
            pub(crate) fn ${deserFnName(shape)}(
                deserializer: &mut dyn ::aws_smithy_schema::serde::ShapeDeserializer,
            ) -> ::std::result::Result<${esc(parse)}, ::aws_smithy_schema::serde::SerdeError> {
                ##[allow(unused_mut)]
                let mut builder = ${esc(builderPath)}::default();
                ##[allow(unused_variables, unreachable_code, clippy::single_match, clippy::match_single_binding)]
                deserializer.read_struct(&${schemaPrefix}_SCHEMA, &mut |member, deser| {
                    match member.member_index() {
                        ${esc(arms.toString())}
                        _ => {}
                    }
                    Ok(())
                })?;
                Ok($result)
            }
            """,
        )
    }

    private fun renderUnionDeserFn(shape: UnionShape) {
        val symbol = symbolProvider.toSymbol(shape)
        val schemaPrefix = symbol.name.uppercase()
        val canReach = canReachConstrained(shape)
        val parse = parseSymbolFullName(shape)
        val members = shape.allMembers.values.toList()

        val arms = StringBuilder()
        members.forEachIndexed { idx, member ->
            val target = model.expectShape(member.target)
            val variantName =
                if (canReach) {
                    unconstrainedShapeSymbolProvider.toMemberName(member)
                } else {
                    symbolProvider.toMemberName(member)
                }
            if (member.isTargetUnit()) {
                arms.append(
                    """
                    Some($idx) => { deser.read_struct(member, &mut |_, _| Ok(()))?; ${esc(parse)}::$variantName },
                    """,
                )
            } else {
                val expr = readTargetExpr(target, "member")
                val boxed =
                    if (canReach) {
                        unconstrainedShapeSymbolProvider.toSymbol(member).isRustBoxed()
                    } else {
                        symbolProvider.toSymbol(member).isRustBoxed()
                    }
                val value =
                    if (boxed) {
                        "::std::boxed::Box::new($expr.into())"
                    } else {
                        "$expr.into()"
                    }
                arms.append(
                    """
                    Some($idx) => ${esc(parse)}::$variantName($value),
                    """,
                )
            }
        }

        // Server semantics: no `Unknown` arm — an unrecognized variant, a null variant
        // value, or an empty union document is a wire-level error (protocol 4xx).
        writer.rust(
            """
            ##[allow(clippy::needless_question_mark)]
            pub(crate) fn ${deserFnName(shape)}(
                deserializer: &mut dyn ::aws_smithy_schema::serde::ShapeDeserializer,
            ) -> ::std::result::Result<${esc(parse)}, ::aws_smithy_schema::serde::SerdeError> {
                let mut result: ::std::option::Option<${esc(parse)}> = ::std::option::Option::None;
                ##[allow(unused_variables, unreachable_code, clippy::single_match, clippy::match_single_binding)]
                deserializer.read_struct(&${schemaPrefix}_SCHEMA, &mut |member, deser| {
                    // Legacy parity: a null variant value is skipped (the union stays
                    // unset), not an error.
                    if deser.is_null() {
                        deser.read_null()?;
                        return Ok(());
                    }
                    // A union document may set exactly one variant (the Smithy
                    // malformed-union protocol tests pin this; legacy errored with
                    // "encountered mixed variants in union").
                    if result.is_some() {
                        return Err(::aws_smithy_schema::serde::SerdeError::custom("encountered mixed variants in union"));
                    }
                    result = ::std::option::Option::Some(match member.member_index() {
                        ${esc(arms.toString())}
                        _ => return Err(::aws_smithy_schema::serde::SerdeError::custom("unknown union variant")),
                    });
                    Ok(())
                })?;
                result.ok_or_else(|| ::aws_smithy_schema::serde::SerdeError::custom("expected a union variant"))
            }
            """,
        )
    }

    /**
     * For an operation input shape: implements the runtime seam
     * [`DeserializableShape`], delegating to the walker and running the single
     * top-level `build()`. Constraint violations convert through the generated
     * protocol-free `From<ConstraintViolation> for DeserializeError` (which boxes
     * the modeled validation error, plan 2d).
     */
    fun renderDeserializableShapeImpl() {
        check(shape is StructureShape) { "DeserializableShape is implemented for operation input structs only" }
        val symbol = symbolProvider.toSymbol(shape)
        val fallible = canReachConstrained(shape)
        val body =
            if (fallible) {
                """
                let builder = ${deserFnName(shape)}(deserializer)?;
                builder.build().map_err(::std::convert::Into::into)
                """
            } else {
                """
                Ok(${deserFnName(shape)}(deserializer)?)
                """
            }
        writer.rustTemplate(
            """
            impl #{DeserializableShape} for ${symbol.fullName} {
                fn deserialize(
                    deserializer: &mut dyn #{ShapeDeserializer},
                ) -> ::std::result::Result<Self, #{DeserializeError}> {
                    ${esc(body)}
                }
            }
            """,
            "DeserializableShape" to
                ServerCargoDependency.smithyHttpServer(runtimeConfig).toType()
                    .resolve("deserialize::DeserializableShape"),
            "DeserializeError" to
                ServerCargoDependency.smithyHttpServer(runtimeConfig).toType()
                    .resolve("deserialize::DeserializeError"),
            "ShapeDeserializer" to
                software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.smithySchema(runtimeConfig)
                    .resolve("serde::ShapeDeserializer"),
        )
    }
}
