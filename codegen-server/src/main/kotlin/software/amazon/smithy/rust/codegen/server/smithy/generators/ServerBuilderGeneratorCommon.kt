/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.node.ArrayNode
import software.amazon.smithy.model.node.BooleanNode
import software.amazon.smithy.model.node.NullNode
import software.amazon.smithy.model.node.NumberNode
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.model.node.StringNode
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
import software.amazon.smithy.model.shapes.NumberShape
import software.amazon.smithy.model.shapes.ShortShape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.TimestampShape
import software.amazon.smithy.model.traits.DefaultTrait
import software.amazon.smithy.model.traits.EnumDefinition
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.generators.EnumMemberModel
import software.amazon.smithy.rust.codegen.core.util.UNREACHABLE
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.expectTrait
import software.amazon.smithy.rust.codegen.core.util.isStreaming
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.hasPublicConstrainedWrapperTupleType

/**
 * Some common freestanding functions shared across:
 *     - [ServerBuilderGenerator]; and
 *     - [ServerBuilderGeneratorWithoutPublicConstrainedTypes],
 * to keep them DRY and consistent.
 */

/**
 * Returns a writable to render the return type of the server builders' `build()` method.
 */
fun buildFnReturnType(isBuilderFallible: Boolean, structureSymbol: Symbol) = writable {
    if (isBuilderFallible) {
        rust("Result<#T, ConstraintViolation>", structureSymbol)
    } else {
        rust("#T", structureSymbol)
    }
}

/**
 * Renders code to fall back to the modeled `@default` value on a [member] shape.
 * The code is expected to be interpolated right after a value of type `Option<T>`, where `T` is the type of the
 * default value.
 */
fun generateFallbackCodeToDefaultValue(
    writer: RustWriter,
    member: MemberShape,
    model: Model,
    runtimeConfig: RuntimeConfig,
    symbolProvider: RustSymbolProvider,
    publicConstrainedTypes: Boolean,
) {
    val defaultValue = defaultValue(model, runtimeConfig, symbolProvider, member)
    val targetShape = model.expectShape(member.target)

    if (member.isStreaming(model)) {
        writer.rust(".unwrap_or_default()")
    } else if (targetShape.hasPublicConstrainedWrapperTupleType(model, publicConstrainedTypes)) {
        // TODO(https://github.com/awslabs/smithy-rs/issues/2134): Instead of panicking here, which will ungracefully
        //  shut down the service, perform the `try_into()` check _once_ at service startup time, perhaps
        //  storing the result in a `OnceCell` that could be reused.
        writer.rustTemplate(
            """
            .unwrap_or_else(||
                #{DefaultValue:W}
                    .try_into()
                    .expect("this check should have failed at generation time; please file a bug report under https://github.com/awslabs/smithy-rs/issues")
            )
            """,
            "DefaultValue" to defaultValue,
        )
    } else {
        when (targetShape) {
            is NumberShape, is EnumShape, is BooleanShape -> {
                writer.rustTemplate(".unwrap_or(#{DefaultValue:W})", "DefaultValue" to defaultValue)
            }
            // Values for the Rust types of the rest of the shapes require heap allocations, so we calculate them
            // in a (lazily-executed) closure for slight performance gains.
            else -> {
                writer.rustTemplate(".unwrap_or_else(|| #{DefaultValue:W})", "DefaultValue" to defaultValue)
            }
        }
    }
}

/**
 * Returns a writable to construct a Rust value of the correct type holding the modeled `@default` value on the
 * [member] shape.
 */
fun defaultValue(
    model: Model,
    runtimeConfig: RuntimeConfig,
    symbolProvider: RustSymbolProvider,
    member: MemberShape,
) = writable {
    val node = member.expectTrait<DefaultTrait>().toNode()!!
    val types = ServerCargoDependency.smithyTypes(runtimeConfig).toType()
    // Define the exception once for DRYness.
    val unsupportedDefaultValueException =
        CodegenException("Default value $node for member shape ${member.id} is unsupported or cannot exist; please file a bug report under https://github.com/awslabs/smithy-rs/issues")
    when (val target = model.expectShape(member.target)) {
        is EnumShape, is IntEnumShape -> {
            val value = when (target) {
                is IntEnumShape -> node.expectNumberNode().value
                is EnumShape -> node.expectStringNode().value
                else -> throw CodegenException("Default value for shape ${target.id} must be of EnumShape or IntEnumShape")
            }
            val enumValues = when (target) {
                is IntEnumShape -> target.enumValues
                is EnumShape -> target.enumValues
                else -> UNREACHABLE(
                    "Target shape ${target.id} must be an `EnumShape` or an `IntEnumShape` at this point, otherwise it would have failed above",
                )
            }
            val variant = enumValues
                .entries
                .filter { entry -> entry.value == value }
                .map { entry ->
                    EnumMemberModel.toEnumVariantName(
                        symbolProvider,
                        target,
                        EnumDefinition.builder().name(entry.key).value(entry.value.toString()).build(),
                    )!!
                }
                .first()
            rust("#T::${variant.name}", symbolProvider.toSymbol(target))
        }

        is ByteShape -> rust(node.expectNumberNode().value.toString() + "i8")
        is ShortShape -> rust(node.expectNumberNode().value.toString() + "i16")
        is IntegerShape -> rust(node.expectNumberNode().value.toString() + "i32")
        is LongShape -> rust(node.expectNumberNode().value.toString() + "i64")
        is FloatShape -> rust(node.expectNumberNode().value.toFloat().toString() + "f32")
        is DoubleShape -> rust(node.expectNumberNode().value.toDouble().toString() + "f64")
        is BooleanShape -> rust(node.expectBooleanNode().value.toString())
        is StringShape -> rust("String::from(${node.expectStringNode().value.dq()})")
        is TimestampShape -> when (node) {
            is NumberNode -> rust(node.expectNumberNode().value.toString())
            is StringNode -> {
                val value = node.expectStringNode().value
                rustTemplate(
                    """
                    #{SmithyTypes}::DateTime::from_str("$value", #{SmithyTypes}::date_time::Format::DateTime)
                            .expect("default value `$value` cannot be parsed into a valid date time; please file a bug report under https://github.com/awslabs/smithy-rs/issues")
                    """,
                    "SmithyTypes" to types,
                )
            }
            else -> throw unsupportedDefaultValueException
        }
        is ListShape -> {
            check(node is ArrayNode && node.isEmpty)
            rust("Vec::new()")
        }
        is MapShape -> {
            check(node is ObjectNode && node.isEmpty)
            rust("std::collections::HashMap::new()")
        }
        is DocumentShape -> {
            when (node) {
                is NullNode -> rustTemplate(
                    "#{SmithyTypes}::Document::Null",
                    "SmithyTypes" to types,
                )

                is BooleanNode -> rustTemplate("""#{SmithyTypes}::Document::Bool(${node.value})""", "SmithyTypes" to types)
                is StringNode -> rustTemplate("#{SmithyTypes}::Document::String(String::from(${node.value.dq()}))", "SmithyTypes" to types)
                is NumberNode -> {
                    val value = node.value.toString()
                    val variant = when (node.value) {
                        is Float, is Double -> "Float"
                        else -> if (node.value.toLong() >= 0) "PosInt" else "NegInt"
                    }
                    rustTemplate(
                        "#{SmithyTypes}::Document::Number(#{SmithyTypes}::Number::$variant($value))",
                        "SmithyTypes" to types,
                    )
                }

                is ArrayNode -> {
                    check(node.isEmpty)
                    rustTemplate("""#{SmithyTypes}::Document::Array(Vec::new())""", "SmithyTypes" to types)
                }

                is ObjectNode -> {
                    check(node.isEmpty)
                    rustTemplate("#{SmithyTypes}::Document::Object(std::collections::HashMap::new())", "SmithyTypes" to types)
                }

                else -> throw unsupportedDefaultValueException
            }
        }

        is BlobShape -> rust("Default::default()")

        else -> throw unsupportedDefaultValueException
    }
}
