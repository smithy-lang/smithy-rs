/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.knowledge.TopDownIndex
import software.amazon.smithy.model.shapes.BigDecimalShape
import software.amazon.smithy.model.shapes.BigIntegerShape
import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.shapes.BooleanShape
import software.amazon.smithy.model.shapes.ByteShape
import software.amazon.smithy.model.shapes.CollectionShape
import software.amazon.smithy.model.shapes.DocumentShape
import software.amazon.smithy.model.shapes.DoubleShape
import software.amazon.smithy.model.shapes.EnumShape
import software.amazon.smithy.model.shapes.FloatShape
import software.amazon.smithy.model.shapes.IntEnumShape
import software.amazon.smithy.model.shapes.IntegerShape
import software.amazon.smithy.model.shapes.LongShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
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
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.isOptional
import software.amazon.smithy.rust.codegen.core.smithy.isRustBoxed
import software.amazon.smithy.rust.codegen.core.smithy.rustType
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.isEventStream
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.canReachConstrainedShape
import software.amazon.smithy.rust.codegen.server.smithy.targetCanReachConstrainedShape
import software.amazon.smithy.rust.codegen.server.smithy.traits.isReachableFromOperationInput

/**
 * Renders the schema-driven walker that reads a structure or union from a `ShapeDeserializer`.
 *
 * The walker is what the generated `FromRequest` impl feeds the protocol's request deserializer into. It is
 * transport-blind: it drives `read_struct` on the shape's `SCHEMA` and, for each member the deserializer
 * yields, reads the value and **assigns the builder field directly**. Members never go through the by-value
 * `set_*` setters: inside the per-member closure those would move the whole builder on every member, which
 * measured at 8-13% of the request cost on realistic inputs.
 *
 * Constraint validation does not move. A shape that can reach a constrained shape is read into its
 * *unconstrained* representation, exactly as the JSON/CBOR parsers do: nested structures come back as their
 * `Builder`, collections and unions as their `*Unconstrained` types, enums as `String`. Those flow into the
 * builder fields as `MaybeConstrained::Unconstrained` through the existing `From` impls, and the operation
 * input's `build()` validates everything at once, so `ValidationException` field paths stay identical to the
 * legacy path.
 *
 * Only shapes reachable from an operation input get a walker; nothing deserializes outputs or errors on the
 * server. Structures with event-stream or streaming-blob members keep the legacy serde and get no walker.
 */
class ServerSchemaDeserializeGenerator(
    private val codegenContext: ServerCodegenContext,
    private val writer: RustWriter,
    private val shape: Shape,
    private val validationExceptionConversionGenerator: ValidationExceptionConversionGenerator,
) {
    private val model = codegenContext.model
    private val symbolProvider = codegenContext.symbolProvider
    private val unconstrainedShapeSymbolProvider = codegenContext.unconstrainedShapeSymbolProvider
    private val smithySchema = RuntimeType.smithySchema(codegenContext.runtimeConfig)
    private val codegenScope =
        arrayOf(
            "ShapeDeserializer" to smithySchema.resolve("serde::ShapeDeserializer"),
            "SerdeError" to smithySchema.resolve("serde::SerdeError"),
            "prelude" to smithySchema.resolve("prelude"),
        )

    /** The name of the generated method. */
    val functionName = FUNCTION_NAME

    fun render() {
        when (shape) {
            is StructureShape ->
                if (shape.isReachableFromOperationInput() && !hasStreamingMember(shape)) {
                    renderStructure(shape)
                }
            is UnionShape ->
                if (shape.isReachableFromOperationInput() && !shape.hasTrait<StreamingTrait>()) {
                    renderUnion(shape)
                }
            else -> {}
        }
    }

    /** Implements the runtime entry point on non-streaming operation input structures. */
    fun renderDeserializableShapeImpl() {
        if (shape !is StructureShape || hasStreamingMember(shape)) return
        val isOperationInput =
            TopDownIndex.of(model).getContainedOperations(codegenContext.serviceShape)
                .any { it.inputShape(model).id == shape.id }
        if (!isOperationInput) return
        val symbol = symbolProvider.toSymbol(shape)
        val constrained = shape.canReachConstrainedShape(model, symbolProvider)
        writer.rustTemplate(
            """
            impl #{DeserializableShape} for ${symbol.name} {
                fn deserialize(deserializer: &mut dyn #{ShapeDeserializer}) -> ::std::result::Result<Self, #{DeserializeError}> {
                    let value = Self::$FUNCTION_NAME(deserializer)?;
                    #{finish:W}
                }
            }
            """,
            *codegenScope,
            "DeserializableShape" to ServerCargoDependency.smithyHttpServer(codegenContext.runtimeConfig).toType().resolve("schema::DeserializableShape"),
            "DeserializeError" to ServerCargoDependency.smithyHttpServer(codegenContext.runtimeConfig).toType().resolve("schema::DeserializeError"),
            "finish" to
                writable {
                    if (constrained) {
                        // A constraint violation becomes the modeled validation exception, exactly as the legacy
                        // `RequestRejection` path does, so the protocol renders it with its message and field list.
                        rustTemplate(
                            """
                            value.build().map_err(|constraint_violation| {
                                #{DeserializeError}::ConstraintViolation(
                                    Box::new(#{ValidationException}::from(constraint_violation)),
                                )
                            })
                            """,
                            "DeserializeError" to ServerCargoDependency.smithyHttpServer(codegenContext.runtimeConfig).toType().resolve("schema::DeserializeError"),
                            "ValidationException" to validationExceptionConversionGenerator.validationExceptionSymbol(),
                        )
                    } else {
                        rust("Ok(value)")
                    }
                },
        )
    }

    private fun hasStreamingMember(shape: StructureShape): Boolean =
        shape.members().any { member ->
            member.isEventStream(model) || model.expectShape(member.target).hasTrait<StreamingTrait>()
        }

    /** The type the walker returns: the unconstrained representation when the shape needs validation. */
    private fun returnSymbol(shape: Shape) =
        if (shape.canReachConstrainedShape(model, symbolProvider)) {
            unconstrainedShapeSymbolProvider.toSymbol(shape)
        } else {
            symbolProvider.toSymbol(shape)
        }

    private fun renderStructure(shape: StructureShape) {
        val symbol = symbolProvider.toSymbol(shape)
        val returnSymbol = returnSymbol(shape)
        val members = shape.allMembers.values.toList()
        val builderSymbol = shape.serverBuilderSymbol(codegenContext)
        val unconstrained = shape.canReachConstrainedShape(model, symbolProvider)

        writer.rustTemplate(
            """
            impl ${symbol.name} {
                /// Reads this shape from a [`ShapeDeserializer`](#{ShapeDeserializer}) guided by [`Self::SCHEMA`].
                ///
                /// Returns the ${if (unconstrained) "unconstrained builder; `build()` validates the modeled constraints" else "shape"}.
                ##[allow(dead_code, unused_variables, unused_mut, clippy::match_single_binding, clippy::single_match)]
                pub(crate) fn $FUNCTION_NAME(
                    deserializer: &mut dyn #{ShapeDeserializer},
                ) -> ::std::result::Result<#{Return}, #{SerdeError}> {
                    let mut builder = #{Builder}::default();
                    deserializer.read_struct(Self::SCHEMA, &mut |member, deser| {
                        match member.member_index() {
                            #{arms}
                            _ => {}
                        }
                        Ok(())
                    })?;
                    #{finish}
                }
            }
            """,
            *codegenScope,
            "Return" to returnSymbol,
            "Builder" to builderSymbol,
            "arms" to
                writable {
                    members.forEachIndexed { index, member ->
                        val fieldName = symbolProvider.toMemberName(member)
                        val assignment = "builder.$fieldName = Some(${memberValueExpr(member)});"
                        if (symbolProvider.toSymbol(member).isOptional()) {
                            rust("Some($index) => { if deser.is_null() { deser.read_null()?; } else { $assignment } }")
                        } else {
                            rust("Some($index) => { $assignment }")
                        }
                    }
                },
            "finish" to
                writable {
                    if (unconstrained) {
                        rust("Ok(builder)")
                    } else {
                        rust("Ok(builder.build())")
                    }
                },
        )
    }

    private fun renderUnion(shape: UnionShape) {
        val symbol = symbolProvider.toSymbol(shape)
        val returnSymbol = returnSymbol(shape)
        val unconstrained = shape.canReachConstrainedShape(model, symbolProvider)
        val members = shape.allMembers.values.toList()

        writer.rustTemplate(
            """
            impl ${symbol.name} {
                /// Reads this union from a [`ShapeDeserializer`](#{ShapeDeserializer}) guided by [`Self::SCHEMA`].
                ##[allow(dead_code, unused_variables, unused_mut, clippy::match_single_binding, clippy::single_match)]
                pub(crate) fn $FUNCTION_NAME(
                    deserializer: &mut dyn #{ShapeDeserializer},
                ) -> ::std::result::Result<#{Return}, #{SerdeError}> {
                    let mut value: ::std::option::Option<#{Return}> = None;
                    deserializer.read_struct(Self::SCHEMA, &mut |member, deser| {
                        match member.member_index() {
                            #{arms}
                            _ => {}
                        }
                        Ok(())
                    })?;
                    value.ok_or_else(|| #{SerdeError}::custom("expected exactly one union variant to be set"))
                }
            }
            """,
            *codegenScope,
            "Return" to returnSymbol,
            "arms" to
                writable {
                    members.forEachIndexed { index, member ->
                        val variant =
                            if (unconstrained) {
                                unconstrainedShapeSymbolProvider.toMemberName(member)
                            } else {
                                symbolProvider.toMemberName(member)
                            }
                        val target = model.expectShape(member.target)
                        val returnType = returnSymbol.rustType().qualifiedName()
                        if (target.id == UNIT) {
                            rust("Some($index) => { deser.read_struct(member, &mut |_, _| Ok(()))?; value = Some($returnType::$variant); }")
                        } else {
                            rust("Some($index) => { value = Some($returnType::$variant(${memberValueExpr(member)})); }")
                        }
                    }
                },
        )
    }

    /**
     * The expression that produces the value stored for [member]: the read expression, converted into
     * `MaybeConstrained` when the target can reach a constrained shape (the same `.into()` the JSON parser's
     * `ServerRequestBeforeBoxingDeserializedMemberConvertToMaybeConstrained*` customizations apply), and boxed
     * for recursive members, in that order (mirroring `ServerBuilderGenerator.builderMemberSymbol`).
     */
    private fun memberValueExpr(member: MemberShape): String {
        val target = model.expectShape(member.target)
        var expr = readExpr(target, "member")
        if (member.targetCanReachConstrainedShape(model, symbolProvider)) {
            expr = "($expr).into()"
        }
        if (symbolProvider.toSymbol(member).isRustBoxed()) {
            expr = "Box::new($expr)"
        }
        return expr
    }

    /**
     * The read expression for a value of shape [target] whose schema is reachable as [schemaRef]. Collections
     * that can reach a constrained shape are wrapped in their `*Unconstrained` newtype so the result matches
     * what the builder field (or the enclosing collection) expects.
     */
    private fun readExpr(
        target: Shape,
        schemaRef: String,
    ): String =
        when (target) {
            is BooleanShape -> "deser.read_boolean($schemaRef)?"
            is ByteShape -> "deser.read_byte($schemaRef)?"
            is ShortShape -> "deser.read_short($schemaRef)?"
            is IntegerShape, is IntEnumShape -> "deser.read_integer($schemaRef)?"
            is LongShape -> "deser.read_long($schemaRef)?"
            is FloatShape -> "deser.read_float($schemaRef)?"
            is DoubleShape -> "deser.read_double($schemaRef)?"
            is BigIntegerShape -> "deser.read_big_integer($schemaRef)?"
            is BigDecimalShape -> "deser.read_big_decimal($schemaRef)?"
            // Server enums are closed and therefore constrained: the unconstrained value is the raw string.
            is EnumShape, is StringShape -> "deser.read_string($schemaRef)?"
            is BlobShape -> "deser.read_blob($schemaRef)?"
            is TimestampShape -> "deser.read_timestamp($schemaRef)?"
            is DocumentShape -> "deser.read_document($schemaRef)?"
            is StructureShape, is UnionShape -> {
                val name = symbolProvider.toSymbol(target).rustType().qualifiedName()
                "$name::$FUNCTION_NAME(deser)?"
            }
            is CollectionShape -> wrapUnconstrained(target, listReadExpr(target, schemaRef))
            is MapShape -> wrapUnconstrained(target, mapReadExpr(target, schemaRef))
            else -> throw IllegalArgumentException("cannot deserialize shape ${target.id} from a schema")
        }

    private fun wrapUnconstrained(
        target: Shape,
        expr: String,
    ): String =
        if (target.canReachConstrainedShape(model, symbolProvider)) {
            "${unconstrainedShapeSymbolProvider.toSymbol(target).rustType().qualifiedName()}($expr)"
        } else {
            expr
        }

    private fun listReadExpr(
        target: CollectionShape,
        schemaRef: String,
    ): String {
        val element = model.expectShape(target.member.target)
        val sparse = target.hasTrait<SparseTrait>()
        // The closed set of helpers on `ShapeDeserializer` skips a vtable call per element for the common
        // scalar lists; everything else reads element by element against the list's member schema.
        val helper =
            if (!sparse) {
                when (element) {
                    is StringShape -> if (!isStringEnum(element)) "deser.read_string_list($schemaRef)?" else null
                    is BlobShape -> "deser.read_blob_list($schemaRef)?"
                    is IntegerShape, is IntEnumShape -> "deser.read_integer_list($schemaRef)?"
                    is LongShape -> "deser.read_long_list($schemaRef)?"
                    else -> null
                }
            } else {
                null
            }
        if (helper != null) {
            return helper
        }
        val elementRead = sparseAware(sparse, readExpr(element, "element"))
        // Nested structures and unions read against their own `SCHEMA`; only scalars and nested
        // collections need the list's member schema.
        val bindElement =
            if (elementRead.contains("element")) {
                "let element = $schemaRef.member().unwrap_or(${preludeFallback(element)}); "
            } else {
                ""
            }
        return "{ ${bindElement}let mut container = ::std::vec::Vec::new(); " +
            "deser.read_list($schemaRef, &mut |deser| { container.push($elementRead); Ok(()) })?; container }"
    }

    private fun mapReadExpr(
        target: MapShape,
        schemaRef: String,
    ): String {
        val key = model.expectShape(target.key.target)
        val value = model.expectShape(target.value.target)
        val sparse = target.hasTrait<SparseTrait>()
        if (!sparse && !isStringEnum(key) && value is StringShape && !isStringEnum(value)) {
            return "deser.read_string_string_map($schemaRef)?"
        }
        // Keys are always read as `String`: enum keys make the map constrained, so its unconstrained type
        // holds `String` keys and validation converts them.
        val valueRead = sparseAware(sparse, readExpr(value, "value_schema"))
        val bindValueSchema =
            if (valueRead.contains("value_schema")) {
                "let value_schema = $schemaRef.member().unwrap_or(${preludeFallback(value)}); "
            } else {
                ""
            }
        return "{ ${bindValueSchema}let mut container = ::std::collections::HashMap::new(); " +
            "deser.read_map($schemaRef, &mut |key, deser| { container.insert(key, $valueRead); Ok(()) })?; container }"
    }

    private fun sparseAware(
        sparse: Boolean,
        readExpr: String,
    ): String =
        if (sparse) {
            "if deser.is_null() { deser.read_null()?; None } else { Some($readExpr) }"
        } else {
            readExpr
        }

    /** A prelude schema to read against if a generated collection schema ever lacks its member schema. */
    private fun preludeFallback(shape: Shape): String {
        val prelude = smithySchema.resolve("prelude").fullyQualifiedName()
        val name =
            when (shape) {
                is BooleanShape -> "BOOLEAN"
                is ByteShape -> "BYTE"
                is ShortShape -> "SHORT"
                is IntegerShape, is IntEnumShape -> "INTEGER"
                is LongShape -> "LONG"
                is FloatShape -> "FLOAT"
                is DoubleShape -> "DOUBLE"
                is BigIntegerShape -> "BIG_INTEGER"
                is BigDecimalShape -> "BIG_DECIMAL"
                is StringShape, is EnumShape -> "STRING"
                is BlobShape -> "BLOB"
                is TimestampShape -> "TIMESTAMP"
                else -> "DOCUMENT"
            }
        return "&$prelude::$name"
    }

    private fun isStringEnum(shape: Shape): Boolean = shape is EnumShape || shape.hasTrait<EnumTrait>()

    companion object {
        /** The name of the generated walker method on structures and unions. */
        const val FUNCTION_NAME = "deserialize_schema"
        private val UNIT: ShapeId = ShapeId.from("smithy.api#Unit")
    }
}
