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
import software.amazon.smithy.model.shapes.IntegerShape
import software.amazon.smithy.model.shapes.ListShape
import software.amazon.smithy.model.shapes.LongShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.NumberShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShortShape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.TimestampShape
import software.amazon.smithy.model.traits.DefaultTrait
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.map
import software.amazon.smithy.rust.codegen.core.rustlang.qualifiedName
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.generators.PrimitiveInstantiator
import software.amazon.smithy.rust.codegen.core.smithy.rustType
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.expectTrait
import software.amazon.smithy.rust.codegen.core.util.isStreaming
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.hasPublicConstrainedWrapperTupleType

/*
 * Some common freestanding functions shared across:
 *     - [ServerBuilderGenerator]; and
 *     - [ServerBuilderGeneratorWithoutPublicConstrainedTypes],
 * to keep them DRY and consistent.
 */

/**
 * Returns a writable to render the return type of the server builders' `build()` method.
 */
fun buildFnReturnType(
    isBuilderFallible: Boolean,
    structureSymbol: Symbol,
    lifetime: String,
) = writable {
    if (isBuilderFallible) {
        rust("Result<#T $lifetime, ConstraintViolation>", structureSymbol)
    } else {
        rust("#T $lifetime", structureSymbol)
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
    val targetShape = model.expectShape(member.target)
    // TODO(https://github.com/rust-lang/rust-clippy/issues/14789):
    // Temporary fix for `#[allow(clippy::redundant_closure)]` not working in `rustc` version 1.82.
    // The issue occurs specifically when we generate code in the form:
    // ```rust
    // .unwrap_or_else(HashMap::new())
    // ```
    // Instead of the linter suggested code:
    // ```rust
    // .unwrap_or_default()
    // ```
    if (isTargetListOrMap(targetShape, member)) {
        writer.rustTemplate(".unwrap_or_default()")
        return
    }

    var defaultValue = defaultValue(model, runtimeConfig, symbolProvider, member)
    val targetSymbol = symbolProvider.toSymbol(targetShape)
    // We need an .into() conversion to create defaults for the server types. A larger scale refactoring could store this information in the
    // symbol, however, retrieving it in this manner works for the moment.
    if (targetSymbol.rustType().qualifiedName().startsWith("::aws_smithy_http_server_python")) {
        defaultValue = defaultValue.map { rust("#T.into()", it) }
    }

    if (member.isStreaming(model)) {
        writer.rust(".unwrap_or_default()")
    } else if (targetShape.hasPublicConstrainedWrapperTupleType(model, publicConstrainedTypes)) {
        // TODO(https://github.com/smithy-lang/smithy-rs/issues/2134): Instead of panicking here, which will ungracefully
        //  shut down the service, perform the `try_into()` check _once_ at service startup time, perhaps
        //  storing the result in a `OnceCell` that could be reused.
        writer.rustTemplate(
            """
            .unwrap_or_else(||
                #{DefaultValue:W}
                    .try_into()
                    .expect("this check should have failed at generation time; please file a bug report under https://github.com/smithy-lang/smithy-rs/issues")
            )
            """,
            "DefaultValue" to defaultValue,
        )
    } else {
        val node = member.expectTrait<DefaultTrait>().toNode()!!
        if ((targetShape is DocumentShape && (node is BooleanNode || node is NumberNode)) ||
            targetShape is BooleanShape ||
            targetShape is NumberShape ||
            targetShape is EnumShape
        ) {
            writer.rustTemplate(".unwrap_or(#{DefaultValue:W})", "DefaultValue" to defaultValue)
        } else {
            // Values for the Rust types of the rest of the shapes might require heap allocations,
            // so we calculate them in a (lazily-executed) closure for minimal performance gains.
            writer.rustTemplate(".unwrap_or_else(##[allow(clippy::redundant_closure)] || #{DefaultValue:W})", "DefaultValue" to defaultValue)
        }
    }
}

private fun isTargetListOrMap(
    targetShape: Shape?,
    member: MemberShape,
): Boolean {
    if (targetShape is ListShape) {
        val node = member.expectTrait<DefaultTrait>().toNode()!!
        check(node is ArrayNode && node.isEmpty)
        return true
    } else if (targetShape is MapShape) {
        val node = member.expectTrait<DefaultTrait>().toNode()!!
        check(node is ObjectNode && node.isEmpty)
        return true
    }
    return false
}

/**
 * Returns a writable to construct a Rust value of the correct type holding the modeled `@default` value on the
 * [member] shape.
 */
private fun defaultValue(
    model: Model,
    runtimeConfig: RuntimeConfig,
    symbolProvider: RustSymbolProvider,
    member: MemberShape,
) = writable {
    val node = member.expectTrait<DefaultTrait>().toNode()!!
    val types = ServerCargoDependency.smithyTypes(runtimeConfig).toType()
    // Define the exception once for DRYness.
    val unsupportedDefaultValueException =
        CodegenException("Default value $node for member shape ${member.id} is unsupported or cannot exist; please file a bug report under https://github.com/smithy-lang/smithy-rs/issues")
    when (val target = model.expectShape(member.target)) {
        is EnumShape -> PrimitiveInstantiator(runtimeConfig, symbolProvider).instantiate(target, node)(this)

        is ByteShape -> rust(node.expectNumberNode().value.toString() + "i8")
        is ShortShape -> rust(node.expectNumberNode().value.toString() + "i16")
        is IntegerShape -> rust(node.expectNumberNode().value.toString() + "i32")
        is LongShape -> rust(node.expectNumberNode().value.toString() + "i64")
        is FloatShape -> rust(node.expectNumberNode().value.toFloat().toString() + "f32")
        is DoubleShape -> rust(node.expectNumberNode().value.toDouble().toString() + "f64")
        is BooleanShape -> rust(node.expectBooleanNode().value.toString())
        is StringShape -> rust("String::from(${node.expectStringNode().value.dq()})")
        is TimestampShape ->
            when (node) {
                is NumberNode -> PrimitiveInstantiator(runtimeConfig, symbolProvider).instantiate(target, node)(this)
                is StringNode -> {
                    val value = node.expectStringNode().value
                    rustTemplate(
                        """
                        #{SmithyTypes}::DateTime::from_str("$value", #{SmithyTypes}::date_time::Format::DateTime)
                                .expect("default value `$value` cannot be parsed into a valid date time; please file a bug report under https://github.com/smithy-lang/smithy-rs/issues")
                        """,
                        "SmithyTypes" to types,
                    )
                }
                else -> throw unsupportedDefaultValueException
            }
        is ListShape -> {
            check(node is ArrayNode && node.isEmpty)
            rustTemplate("#{Vec}::new()", *preludeScope)
        }

        is MapShape -> {
            check(node is ObjectNode && node.isEmpty)
            rustTemplate("#{HashMap}::new()", "HashMap" to RuntimeType.HashMap)
        }

        is DocumentShape -> {
            when (node) {
                is NullNode ->
                    rustTemplate(
                        "#{SmithyTypes}::Document::Null",
                        "SmithyTypes" to types,
                    )

                is BooleanNode ->
                    rustTemplate(
                        """#{SmithyTypes}::Document::Bool(${node.value})""",
                        "SmithyTypes" to types,
                    )

                is StringNode ->
                    rustTemplate(
                        "#{SmithyTypes}::Document::String(#{String}::from(${node.value.dq()}))",
                        *preludeScope,
                        "SmithyTypes" to types,
                    )

                is NumberNode -> {
                    val value = node.value.toString()
                    val variant =
                        when (node.value) {
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
                    rustTemplate(
                        """#{SmithyTypes}::Document::Array(#{Vec}::new())""",
                        *preludeScope,
                        "SmithyTypes" to types,
                    )
                }

                is ObjectNode -> {
                    check(node.isEmpty)
                    rustTemplate(
                        "#{SmithyTypes}::Document::Object(#{HashMap}::new())",
                        "SmithyTypes" to types,
                        "HashMap" to RuntimeType.HashMap,
                    )
                }

                else -> throw unsupportedDefaultValueException
            }
        }

        is BlobShape -> PrimitiveInstantiator(runtimeConfig, symbolProvider).instantiate(target, node)(this)

        else -> throw unsupportedDefaultValueException
    }
}
