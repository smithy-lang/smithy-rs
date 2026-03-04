/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.client.smithy.generators.waiters

import software.amazon.smithy.jmespath.ExpressionSerializer
import software.amazon.smithy.jmespath.JmespathExpression
import software.amazon.smithy.jmespath.RuntimeType
import software.amazon.smithy.jmespath.ast.AndExpression
import software.amazon.smithy.jmespath.ast.BinaryExpression
import software.amazon.smithy.jmespath.ast.ComparatorExpression
import software.amazon.smithy.jmespath.ast.CurrentExpression
import software.amazon.smithy.jmespath.ast.ExpressionTypeExpression
import software.amazon.smithy.jmespath.ast.FieldExpression
import software.amazon.smithy.jmespath.ast.FilterProjectionExpression
import software.amazon.smithy.jmespath.ast.FlattenExpression
import software.amazon.smithy.jmespath.ast.FunctionExpression
import software.amazon.smithy.jmespath.ast.IndexExpression
import software.amazon.smithy.jmespath.ast.LiteralExpression
import software.amazon.smithy.jmespath.ast.MultiSelectHashExpression
import software.amazon.smithy.jmespath.ast.MultiSelectListExpression
import software.amazon.smithy.jmespath.ast.NotExpression
import software.amazon.smithy.jmespath.ast.ObjectProjectionExpression
import software.amazon.smithy.jmespath.ast.OrExpression
import software.amazon.smithy.jmespath.ast.ProjectionExpression
import software.amazon.smithy.jmespath.ast.SliceExpression
import software.amazon.smithy.jmespath.ast.Subexpression
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.BooleanShape
import software.amazon.smithy.model.shapes.CollectionShape
import software.amazon.smithy.model.shapes.EnumShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.NumberShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.SafeNamer
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.asRef
import software.amazon.smithy.rust.codegen.core.rustlang.plus
import software.amazon.smithy.rust.codegen.core.rustlang.render
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.stripOuter
import software.amazon.smithy.rust.codegen.core.rustlang.withBlock
import software.amazon.smithy.rust.codegen.core.rustlang.withBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.isOptional
import software.amazon.smithy.rust.codegen.core.smithy.rustType
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.orNull
import java.text.NumberFormat

/**
 * This needs to exist since there are computed values where there is no modeled
 * shape for the result of the evaluated expression. For example, the multi-select
 * list `['foo', 'baz']` is a list of string, but it isn't modeled anywhere,
 * so there is no Smithy shape to represent it. However, information is still
 * needed about the original shape in order to correctly generate code.
 */
sealed class TraversedShape {
    abstract val shape: Shape?

    data class Array(override val shape: Shape?, val member: TraversedShape) : TraversedShape()

    data class Object(override val shape: Shape?) : TraversedShape()

    data class Bool(override val shape: Shape?) : TraversedShape()

    data class Enum(override val shape: Shape?) : TraversedShape()

    data class Number(override val shape: Shape?) : TraversedShape()

    data class String(override val shape: Shape?) : TraversedShape()

    companion object {
        fun from(
            model: Model,
            shape: Shape,
        ): TraversedShape =
            when {
                shape is MapShape || shape is StructureShape -> Object(shape)
                shape is CollectionShape -> Array(shape, from(model, model.expectShape(shape.member.target)))
                shape is BooleanShape -> Bool(shape)
                shape is EnumShape || shape.hasTrait<EnumTrait>() -> Enum(shape)
                shape is NumberShape -> Number(shape)
                shape is StringShape -> String(shape)
                else -> throw UnsupportedJmesPathException("Shape type ${shape.type} is not supported in JMESPath expressions in smithy-rs")
            }
    }
}

/**
 * Contains information about the output of a visited [JmespathExpression].
 */
data class GeneratedExpression(
    /** The name of the identifier that this expression's evaluation is placed into */
    val identifier: String,
    /** The TraversedShape for the output of the expression evaluation. */
    val outputShape: TraversedShape,
    /**
     * The Rust type for the evaluated expression.
     *
     * For the most part, the code generator operates on output types rather than output shapes
     * since there will always be an output type, whereas there will only sometimes be an output
     * shape. Output shapes are only really used for handling enums and projections.
     */
    val outputType: RustType,
    /** Writable to output this expression's generated code. */
    val output: Writable,
) {
    internal fun isArray(): Boolean = outputShape is TraversedShape.Array

    internal fun isBool(): Boolean = outputShape is TraversedShape.Bool

    internal fun isEnum(): Boolean = outputShape is TraversedShape.Enum

    internal fun isNumber(): Boolean = outputShape is TraversedShape.Number

    internal fun isString(): Boolean = outputShape is TraversedShape.String

    internal fun isStringOrEnum(): Boolean = isString() || isEnum()

    internal fun isObject(): Boolean = outputShape is TraversedShape.Object

    /** Dereferences this expression if it is a reference. */
    internal fun dereference(namer: SafeNamer): GeneratedExpression =
        if (outputType is RustType.Reference) {
            namer.safeName("_tmp").let { tmp ->
                copy(
                    identifier = tmp,
                    outputType = outputType.member,
                    output =
                        output +
                            writable {
                                rust("let $tmp = *$identifier;")
                            },
                )
            }
        } else {
            this
        }

    /** Converts this expression into a &str. */
    internal fun convertToStrRef(namer: SafeNamer): GeneratedExpression =
        if (outputType.isDoubleReference()) {
            dereference(namer).convertToStrRef(namer)
        } else if (isEnum()) {
            namer.safeName("_tmp").let { tmp ->
                GeneratedExpression(
                    identifier = tmp,
                    outputType = RustType.Reference(null, RustType.Opaque("str")),
                    outputShape = TraversedShape.String(null),
                    output =
                        output +
                            writable {
                                rust("let $tmp = $identifier.as_str();")
                            },
                ).convertToStrRef(namer)
            }
        } else if (!outputType.isString()) {
            namer.safeName("_tmp").let { tmp ->
                GeneratedExpression(
                    identifier = tmp,
                    outputType = RustType.String,
                    outputShape = TraversedShape.String(null),
                    output =
                        output +
                            writable {
                                rust("let $tmp = $identifier.to_string();")
                            },
                ).convertToStrRef(namer)
            }
        } else if (!outputType.isStr()) {
            namer.safeName("_tmp").let { tmp ->
                GeneratedExpression(
                    identifier = tmp,
                    outputType = RustType.Reference(null, RustType.Opaque("str")),
                    outputShape = TraversedShape.String(null),
                    output =
                        output +
                            writable {
                                rust("let $tmp = $identifier.as_str();")
                            },
                ).convertToStrRef(namer)
            }
        } else {
            this
        }

    /** Converts a number expression into a specific number type */
    internal fun convertToNumberPrimitive(
        namer: SafeNamer,
        desiredPrimitive: RustType,
    ): GeneratedExpression {
        check(isNumber() && desiredPrimitive.isNumber()) {
            "this function only works on number types"
        }

        return when {
            desiredPrimitive is RustType.Reference -> convertToNumberPrimitive(namer, desiredPrimitive.member)
            outputType is RustType.Reference -> dereference(namer).convertToNumberPrimitive(namer, desiredPrimitive)
            outputType != desiredPrimitive ->
                namer.safeName("_tmp").let { tmp ->
                    GeneratedExpression(
                        identifier = tmp,
                        outputType = desiredPrimitive,
                        outputShape = this.outputShape,
                        output =
                            output +
                                writable {
                                    rust("let $tmp = $identifier as ${desiredPrimitive.render()};")
                                },
                    )
                }

            else -> this
        }
    }
}

/**
 * Identifier binding for JmesPath expressions.
 */
sealed class TraversalBinding {
    /** The name of this binding in the generated Rust code */
    abstract val rustName: String

    /** The Smithy shape behind this binding */
    abstract val shape: TraversedShape

    /** Binds the given shape to the global namespace such that all its members are globally available */
    data class Global(
        override val rustName: String,
        override val shape: TraversedShape,
    ) : TraversalBinding()

    /** Binds a shape to a name */
    data class Named(
        /** What this binding is referred to in JmesPath expressions */
        val jmespathName: String,
        override val rustName: String,
        override val shape: TraversedShape,
    ) : TraversalBinding()
}

typealias TraversalBindings = List<TraversalBinding>

/**
 * Bag of metadata accessible from the generate* methods that can affect how the resulting Rust code should be generated.
 *
 * [retainOption] determines whether `Option`s are preserved in the context of a projected list.
 * Specifically, when applying selectors (used in multi-select lists) to each entity on the left-hand side of a
 * projection, we want the resulting map function to return `Option<Vec<Option<&T>>>` (when `retainOption` is true)
 * rather than `Option<Vec<&T>>` (when it is false).
 * This distinction is crucial because the latter could incorrectly result in `None` if any of the selectors
 * refer to a field with a `None` value.
 */
data class TraversalContext(val retainOption: Boolean)

/**
 * Indicates a feature that's part of the JmesPath spec, but that we explicitly decided
 * not to support in smithy-rs due to the complexity of code generating it for Rust.
 */
class UnsupportedJmesPathException(msg: String?, what: Throwable? = null) : RuntimeException(msg, what)

/** Code can't be generated for the combination of the Smithy shape and the JmesPath expression. */
class InvalidJmesPathTraversalException(msg: String?, what: Throwable? = null) : RuntimeException(msg, what)

/** This indicates a bug in the code generator itself that should be fixed. */
class JmesPathTraversalCodegenBugException(msg: String?, what: Throwable? = null) : RuntimeException(msg, what)

/**
 * Generates code from a JmesPath expression to traverse generated Smithy shapes.
 *
 * This generator implements a subset of the JmesPath spec since the full spec has more features
 * than are needed for real-world Smithy waiters, and some of those features are very complex
 * to code generate for Rust.
 *
 * Specifically, the following Jmespath features are supported:
 * - Fields
 * - Sub-expressions
 * - Comparisons
 * - Filter projections
 * - Object projections
 * - Multi-select lists (but only when every item in the list is the exact same type)
 * - And/or/not boolean operations
 * - Functions `contains`, `length`, and `keys`.
 */
class RustJmespathShapeTraversalGenerator(
    codegenContext: ClientCodegenContext,
) {
    private val model = codegenContext.model
    private val symbolProvider = codegenContext.symbolProvider
    private val safeNamer = SafeNamer()

    fun generate(
        expr: JmespathExpression,
        bindings: TraversalBindings,
        context: TraversalContext,
    ): GeneratedExpression {
        fun String.attachExpression() =
            this.substringBefore("\nExpression:") + "\nExpression: ${ExpressionSerializer().serialize(expr)}"
        try {
            val result =
                when (expr) {
                    is ComparatorExpression -> generateComparator(expr, bindings, context)
                    is FunctionExpression -> generateFunction(expr, bindings, context)
                    is FieldExpression -> generateField(expr, bindings, context)
                    is LiteralExpression -> generateLiteral(expr)
                    is MultiSelectListExpression -> generateMultiSelectList(expr, bindings, context)
                    is AndExpression -> generateAnd(expr, bindings, context)
                    is OrExpression -> generateOr(expr, bindings, context)
                    is NotExpression -> generateNot(expr, bindings, context)
                    is ObjectProjectionExpression -> generateObjectProjection(expr, bindings, context)
                    is FilterProjectionExpression -> generateFilterProjection(expr, bindings, context)
                    is ProjectionExpression -> generateProjection(expr, bindings, context)
                    is Subexpression -> generateSubexpression(expr, bindings, context)
                    is CurrentExpression -> throw JmesPathTraversalCodegenBugException("current expression must be handled in each expression type that can have one")
                    is ExpressionTypeExpression -> throw UnsupportedJmesPathException("Expression type expressions are not supported by smithy-rs")
                    is IndexExpression -> throw UnsupportedJmesPathException("Index expressions are not supported by smithy-rs")
                    is MultiSelectHashExpression -> throw UnsupportedJmesPathException("Multi-select hash expressions are not supported by smithy-rs")
                    is SliceExpression -> throw UnsupportedJmesPathException("Slice expressions are not supported by smithy-rs")
                    else -> throw UnsupportedJmesPathException("${expr.javaClass.name} expression type not supported by smithy-rs")
                }
            return result.copy(
                output =
                    writable {
                        result.output(this)
                        if (debugMode) {
                            rust("// ${result.identifier} = ${ExpressionSerializer().serialize(expr)}")
                        }
                    },
            )
        } catch (ex: UnsupportedJmesPathException) {
            throw UnsupportedJmesPathException(ex.message?.attachExpression(), ex)
        } catch (ex: InvalidJmesPathTraversalException) {
            throw InvalidJmesPathTraversalException(ex.message?.attachExpression(), ex)
        } catch (ex: Exception) {
            throw JmesPathTraversalCodegenBugException(ex.message?.attachExpression(), ex)
        }
    }

    private fun generateComparator(
        expr: ComparatorExpression,
        bindings: TraversalBindings,
        context: TraversalContext,
    ): GeneratedExpression {
        // When applying a comparator to the left and right operands, both must be non-optional types.
        // For this, we avoid retaining `Option` values, even when `generateComparator` is invoked
        // further down the chain from a projection expression.
        val left = generate(expr.left, bindings, context.copy(retainOption = false))
        val right = generate(expr.right, bindings, context.copy(retainOption = false))
        return generateCompare(safeNamer, left, right, expr.comparator.toString())
    }

    private fun generateFunction(
        expr: FunctionExpression,
        bindings: TraversalBindings,
        context: TraversalContext,
    ): GeneratedExpression {
        val ident = safeNamer.safeName("_ret")
        return when (expr.name) {
            "length" -> {
                if (expr.arguments.size != 1) {
                    throw InvalidJmesPathTraversalException("Length function takes exactly one argument")
                }
                val arg = generate(expr.arguments[0], bindings, context)
                if (!arg.isArray() && !arg.isString()) {
                    throw InvalidJmesPathTraversalException("Argument to `length` function must be a collection or string type")
                }
                GeneratedExpression(
                    identifier = ident,
                    outputType = RustType.Integer(64),
                    outputShape = TraversedShape.Number(null),
                    output =
                        writable {
                            arg.output(this)
                            rust("let $ident = ${arg.identifier}.len() as i64;")
                        },
                )
            }

            "contains" -> {
                if (expr.arguments.size != 2) {
                    throw InvalidJmesPathTraversalException("Contains function takes exactly two arguments")
                }
                val left = generate(expr.arguments[0], bindings, context)
                if (!left.isArray() && !left.isString()) {
                    throw InvalidJmesPathTraversalException("First argument to `contains` function must be a collection or string type")
                }
                if (expr.arguments[1].isLiteralNull()) {
                    throw UnsupportedJmesPathException("Checking for null with `contains` is not supported in smithy-rs")
                }
                val right = generate(expr.arguments[1], bindings, context)
                if (!right.isBool() && !right.isNumber() && !right.isString() && !right.isEnum()) {
                    throw UnsupportedJmesPathException("Checking for anything other than booleans, numbers, strings, or enums in the `contains` function is not supported in smithy-rs")
                }
                if (left.isString()) {
                    return GeneratedExpression(
                        identifier = ident,
                        outputType = RustType.Bool,
                        outputShape = TraversedShape.Bool(null),
                        output =
                            left.output +
                                writable {
                                    val rightStr = right.convertToStrRef(safeNamer).also { it.output(this) }
                                    rust("let $ident = ${left.identifier}.contains(${rightStr.identifier});")
                                },
                    )
                } else {
                    return GeneratedExpression(
                        identifier = ident,
                        outputType = RustType.Bool,
                        outputShape = TraversedShape.Bool(null),
                        output =
                            left.output + right.output +
                                writable {
                                    withBlockTemplate("let $ident = ${left.identifier}.iter().any(|_v| {", "});") {
                                        val compare =
                                            generateCompare(
                                                safeNamer,
                                                GeneratedExpression(
                                                    identifier = "_v",
                                                    outputShape = (left.outputShape as TraversedShape.Array).member,
                                                    outputType =
                                                        RustType.Reference(
                                                            lifetime = null,
                                                            member =
                                                                left.outputType.collectionValue(),
                                                        ),
                                                    output = writable {},
                                                ),
                                                // Clear the output since we already wrote the right and don't want to duplicate it
                                                right.copy(output = writable {}),
                                                "==",
                                            ).also { it.output(this) }

                                        rust(compare.identifier)
                                    }
                                },
                    )
                }
            }

            "keys" -> {
                if (expr.arguments.size != 1) {
                    throw InvalidJmesPathTraversalException("Keys function takes exactly one argument")
                }
                val arg = generate(expr.arguments[0], bindings, context)
                if (!arg.isObject()) {
                    throw InvalidJmesPathTraversalException("Argument to `keys` function must be an object type")
                }
                GeneratedExpression(
                    identifier = ident,
                    outputType = RustType.Vec(RustType.String),
                    outputShape = TraversedShape.Array(null, TraversedShape.String(null)),
                    output =
                        writable {
                            arg.output(this)
                            when (val outputShape = arg.outputShape.shape) {
                                is StructureShape -> {
                                    // Can't iterate a struct in Rust so source the keys from smithy
                                    val keys =
                                        outputShape.allMembers.keys.joinToString(",") { "${it.dq()}.to_string()" }
                                    rust("let $ident = vec![$keys];")
                                }

                                is MapShape -> {
                                    rust("let $ident = ${arg.identifier}.keys().map(Clone::clone).collect::<Vec<String>>();")
                                }

                                else ->
                                    throw UnsupportedJmesPathException("The shape type for an input to the keys function must be a struct or a map, got ${outputShape?.type}")
                            }
                        },
                )
            }

            else -> throw UnsupportedJmesPathException("The `${expr.name}` function is not supported by smithy-rs")
        }
    }

    private fun generateField(
        expr: FieldExpression,
        bindings: TraversalBindings,
        context: TraversalContext,
    ): GeneratedExpression {
        val globalBinding = bindings.find { it is TraversalBinding.Global }
        val namedBinding = bindings.find { it is TraversalBinding.Named && it.jmespathName == expr.name }
        if (namedBinding != null && namedBinding.shape is TraversedShape.Object) {
            // If there's a named binding that matches, then immediately return it
            return GeneratedExpression(
                identifier = namedBinding.rustName,
                outputShape = namedBinding.shape,
                outputType = symbolProvider.toSymbol(namedBinding.shape.shape!!).rustType().asRef(),
                output = writable { },
            )
        } else if (globalBinding != null && globalBinding.shape is TraversedShape.Object) {
            // Otherwise, look in the global binding (if available)
            val member =
                globalBinding.shape.shape?.getMember(expr.name)?.orNull()
                    ?: throw InvalidJmesPathTraversalException("Member `${expr.name}` doesn't exist on ${globalBinding.shape.shape?.id}")
            val memberSym = symbolProvider.toSymbol(member)

            val target = model.expectShape(member.target)
            val targetSym = symbolProvider.toSymbol(target)

            val ident = safeNamer.safeName("_fld")
            if (context.retainOption) {
                return GeneratedExpression(
                    identifier = ident,
                    outputShape = TraversedShape.from(model, target),
                    outputType = RustType.Option(targetSym.rustType().asRef()),
                    output =
                        writable {
                            rustTemplate(
                                if (globalBinding.rustName.startsWith("_fld")) {
                                    if (memberSym.isOptional()) {
                                        // This ensures that `ident` has a type with a single level of `Option`, rather than being
                                        // doubly nested as `Option<Option<...>>`.
                                        "let $ident = ${globalBinding.rustName}.and_then(|v| v.${memberSym.name}.as_ref());"
                                    } else {
                                        "let $ident = ${globalBinding.rustName}.map(|v| &v.${memberSym.name});"
                                    }
                                } else {
                                    if (memberSym.isOptional()) {
                                        "let $ident = ${globalBinding.rustName}.${memberSym.name}.as_ref();"
                                    } else {
                                        "let $ident = #{Some}(&${globalBinding.rustName}.${memberSym.name});"
                                    }
                                },
                                *preludeScope,
                            )
                        },
                )
            } else {
                return GeneratedExpression(
                    identifier = ident,
                    outputShape = TraversedShape.from(model, target),
                    outputType = targetSym.rustType().asRef(),
                    output =
                        writable {
                            rust(
                                if (memberSym.isOptional()) {
                                    "let $ident = ${globalBinding.rustName}.${memberSym.name}.as_ref()?;"
                                } else {
                                    "let $ident = &${globalBinding.rustName}.${memberSym.name};"
                                },
                            )
                        },
                )
            }
        } else if (namedBinding != null || globalBinding != null) {
            throw InvalidJmesPathTraversalException("Cannot look up fields in non-struct shapes")
        } else {
            throw JmesPathTraversalCodegenBugException("Missing jmespath traversal binding for ${expr.name}; available bindings: $bindings")
        }
    }

    private fun generateLiteral(expr: LiteralExpression): GeneratedExpression {
        val (outputShape, outputType) =
            when (expr.type) {
                RuntimeType.BOOLEAN ->
                    TraversedShape.Bool(null) to
                        RustType.Reference(
                            lifetime = null,
                            member = RustType.Bool,
                        )

                RuntimeType.NUMBER ->
                    TraversedShape.Number(null) to
                        RustType.Reference(
                            lifetime = null,
                            member = RustType.Float(64),
                        )

                RuntimeType.STRING ->
                    TraversedShape.String(null) to
                        RustType.Reference(
                            lifetime = null,
                            member = RustType.Opaque("str"),
                        )

                RuntimeType.NULL -> throw UnsupportedJmesPathException("Literal nulls are not supported by smithy-rs")
                else -> throw UnsupportedJmesPathException("Literal expression '${ExpressionSerializer().serialize(expr)}' is not supported by smithy-rs")
            }

        fun fmtFloating(floating: Number) =
            NumberFormat.getInstance().apply { minimumFractionDigits = 1 }.format(floating)

        return safeNamer.safeName("_lit").uppercase().let { ident ->
            GeneratedExpression(
                identifier = ident,
                outputType = outputType,
                outputShape = outputShape,
                output =
                    writable {
                        when (expr.type) {
                            RuntimeType.BOOLEAN -> rust("const $ident: &bool = &${expr.expectBooleanValue()};")
                            RuntimeType.NUMBER -> {
                                rust("const $ident: #T = &${fmtFloating(expr.expectNumberValue())};", outputType)
                            }

                            RuntimeType.STRING -> rust("const $ident: &str = ${expr.expectStringValue().dq()};")
                            else -> throw RuntimeException("unreachable")
                        }
                    },
            )
        }
    }

    private fun generateMultiSelectList(
        expr: MultiSelectListExpression,
        bindings: TraversalBindings,
        context: TraversalContext,
    ): GeneratedExpression {
        val expressions =
            expr.expressions.map { subexpr ->
                generate(subexpr, bindings, context)
            }
        // If we wanted to support mixed-types, we would need to use tuples, add tuple support to RustType,
        // and update supported functions such as `contains` to operate on tuples.
        for (pair in expressions.map { it.outputType }.windowed(2)) {
            if (pair[0] != pair[1]) {
                throw UnsupportedJmesPathException("Mixed-type multi-select lists are not supported by smithy-rs")
            }
        }

        return safeNamer.safeName("_msl").let { ident ->
            GeneratedExpression(
                identifier = ident,
                outputType = RustType.Vec(expressions[0].outputType),
                outputShape = TraversedShape.Array(null, expressions[0].outputShape),
                output =
                    writable {
                        expressions.forEach { it.output(this) }
                        rust("let $ident = vec![${expressions.map { it.identifier }.joinToString(", ")}];")
                    },
            )
        }
    }

    private fun generateAnd(
        expr: AndExpression,
        bindings: TraversalBindings,
        context: TraversalContext,
    ): GeneratedExpression = generateBooleanOp(expr, "&&", bindings, context)

    private fun generateOr(
        expr: OrExpression,
        bindings: TraversalBindings,
        context: TraversalContext,
    ): GeneratedExpression = generateBooleanOp(expr, "||", bindings, context)

    private fun generateBooleanOp(
        expr: BinaryExpression,
        op: String,
        bindings: TraversalBindings,
        context: TraversalContext,
    ): GeneratedExpression {
        val left = generate(expr.left, bindings, context)
        val right = generate(expr.right, bindings, context)
        if (!left.isBool() || !right.isBool()) {
            throw UnsupportedJmesPathException("Applying the `$op` operation doesn't support non-boolean types in smithy-rs")
        }

        return safeNamer.safeName("_bo").let { ident ->
            GeneratedExpression(
                identifier = ident,
                outputType = RustType.Bool,
                outputShape = TraversedShape.Bool(null),
                output =
                    writable {
                        val leftBool = left.dereference(safeNamer).also { it.output(this) }
                        val rightBool = right.dereference(safeNamer).also { it.output(this) }
                        rust("let $ident = ${leftBool.identifier} $op ${rightBool.identifier};")
                    },
            )
        }
    }

    private fun generateNot(
        expr: NotExpression,
        bindings: TraversalBindings,
        context: TraversalContext,
    ): GeneratedExpression {
        val inner = generate(expr.expression, bindings, context)
        if (!inner.isBool()) {
            throw UnsupportedJmesPathException("Negation of a non-boolean type is not supported by smithy-rs")
        }

        return safeNamer.safeName("_not").let { ident ->
            GeneratedExpression(
                identifier = ident,
                outputType = RustType.Bool,
                outputShape = TraversedShape.Bool(null),
                output =
                    inner.output +
                        writable {
                            rust("let $ident = !${inner.identifier};")
                        },
            )
        }
    }

    private fun generateProjection(
        expr: ProjectionExpression,
        bindings: TraversalBindings,
        context: TraversalContext,
    ): GeneratedExpression {
        val maybeFlatten = expr.left
        if (maybeFlatten is SliceExpression) {
            throw UnsupportedJmesPathException("Slice expressions are not supported by smithy-rs")
        }
        val left =
            when (maybeFlatten) {
                is FlattenExpression -> generate(maybeFlatten.expression, bindings, context)
                else -> generate(expr.left, bindings, context)
            }

        // Short-circuit in the case where the projection is unnecessary
        if (left.isArray() && expr.right is CurrentExpression) {
            return left
        }

        val leftTarget =
            (
                left.outputShape as? TraversedShape.Array
                    ?: throw InvalidJmesPathTraversalException("Left side of the flatten projection MUST resolve to a list or set shape")
            ).member
        val leftTargetSym: Any = (leftTarget.shape?.let { symbolProvider.toSymbol(it) }) ?: left.outputType
        val leftBinding = "_v"

        val right =
            generate(
                expr.right,
                listOf(TraversalBinding.Global(leftBinding, leftTarget)),
                context.copy(retainOption = true),
            )

        val (projectionType, flattenNeeded) = projectionType(right)

        return safeNamer.safeName("_prj").let { ident ->
            GeneratedExpression(
                identifier = ident,
                outputShape = right.outputShape,
                outputType = projectionType,
                output =
                    left.output +
                        writable {
                            rust("let $ident = ${left.identifier}.iter()")
                            withBlock(".flat_map(|v| {", "})") {
                                renderMapToProject(this, leftBinding, leftTargetSym, right)
                                rust("map(v)")
                            }
                            if (flattenNeeded) {
                                rust(".flatten()")
                                // Eliminate temporary `Option` introduced by `retainOption = true` above.
                                if (right.outputType.isCollectionOfOptions()) {
                                    rust(".flatten()")
                                }
                            }
                            rustTemplate(".collect::<#{Vec}<_>>();", *preludeScope)
                        },
            )
        }
    }

    private fun generateFilterProjection(
        expr: FilterProjectionExpression,
        bindings: TraversalBindings,
        context: TraversalContext,
    ): GeneratedExpression {
        val left = generate(expr.left, bindings, context)
        if (!left.isArray()) {
            throw UnsupportedJmesPathException("Filter projections can only be done on lists or sets in smithy-rs")
        }

        val leftTarget = (left.outputShape as TraversedShape.Array).member
        val leftTargetSym = symbolProvider.toSymbol(leftTarget.shape)
        val leftBinding = "_v"

        val right =
            if (expr.right is CurrentExpression) {
                left.copy(
                    outputType = left.outputType.collectionValue().asRef(),
                    outputShape = leftTarget,
                    output = writable {},
                )
            } else {
                generate(
                    expr.right,
                    listOf(TraversalBinding.Global(leftBinding, leftTarget)),
                    context.copy(retainOption = true),
                )
            }

        val comparison = generate(expr.comparison, listOf(TraversalBinding.Global("_v", leftTarget)), context)
        if (!comparison.isBool()) {
            throw InvalidJmesPathTraversalException("The filter expression comparison must result in a boolean")
        }

        val (projectionType, flattenNeeded) = projectionType(right)

        return safeNamer.safeName("_fprj").let { ident ->
            GeneratedExpression(
                identifier = ident,
                outputShape = TraversedShape.Array(null, right.outputShape),
                outputType = projectionType,
                output =
                    left.output +
                        writable {
                            rust("let $ident = ${left.identifier}.iter()")
                            withBlock(".filter({", "})") {
                                rustBlockTemplate(
                                    "fn filter(_v: &#{Arg}) -> #{Option}<bool>",
                                    "Arg" to leftTargetSym,
                                    *preludeScope,
                                ) {
                                    val toBool = comparison.dereference(safeNamer).also { it.output(this) }
                                    rustTemplate("#{Some}(${toBool.identifier})", *preludeScope)
                                }
                                rust("|v| filter(v).unwrap_or_default()")
                            }
                            if (expr.right !is CurrentExpression) {
                                withBlock(".flat_map({", "})") {
                                    renderMapToProject(this, leftBinding, leftTargetSym, right)
                                    rust("map")
                                }
                                if (flattenNeeded) {
                                    rust(".flatten()")
                                    // Eliminate temporary `Option` introduced by `retainOption = true` above.
                                    if (right.outputType.isCollectionOfOptions()) {
                                        rust(".flatten()")
                                    }
                                }
                            }
                            rustTemplate(".collect::<#{Vec}<_>>();", *preludeScope)
                        },
            )
        }
    }

    private fun generateObjectProjection(
        expr: ObjectProjectionExpression,
        bindings: TraversalBindings,
        context: TraversalContext,
    ): GeneratedExpression {
        if (expr.left is CurrentExpression) {
            throw UnsupportedJmesPathException("Object projection cannot be done on computed maps in smithy-rs")
        }
        val left = generate(expr.left, bindings, context)
        if (!left.outputType.isMap()) {
            throw UnsupportedJmesPathException("Object projection is only supported on map types in smithy-rs")
        }
        if (left.outputShape.shape == null) {
            throw UnsupportedJmesPathException("Object projection cannot be done on computed maps in smithy-rs")
        }

        val leftTarget = model.expectShape((left.outputShape.shape as MapShape).value.target)
        val leftTargetSym = symbolProvider.toSymbol(leftTarget)
        val leftBinding = "_v"

        val right =
            if (expr.right is CurrentExpression) {
                left.copy(
                    outputType =
                        left.outputType.collectionValue().asRef(),
                    outputShape = TraversedShape.from(model, leftTarget),
                    output = writable {},
                )
            } else {
                generate(
                    expr.right,
                    listOf(TraversalBinding.Global(leftBinding, TraversedShape.from(model, leftTarget))),
                    context.copy(retainOption = true),
                )
            }

        val (projectionType, flattenNeeded) = projectionType(right)

        val ident = safeNamer.safeName("_oprj")
        return GeneratedExpression(
            identifier = ident,
            outputShape = TraversedShape.Array(null, right.outputShape),
            outputType = projectionType,
            output =
                left.output +
                    writable {
                        if (expr.right is CurrentExpression) {
                            rustTemplate("let $ident = ${left.identifier}.values().collect::<#{Vec}<_>>();", *preludeScope)
                        } else {
                            withBlock("let $ident = ${left.identifier}.values().flat_map({", "})") {
                                renderMapToProject(this, leftBinding, leftTargetSym, right)
                                rust("map")
                            }
                            if (flattenNeeded) {
                                rust(".flatten()")
                                if (right.outputType.isCollectionOfOptions()) {
                                    // Eliminate temporary `Option` introduced by `retainOption = true` above.
                                    rust(".flatten()")
                                }
                            }
                            rustTemplate(".collect::<#{Vec}<_>>();", *preludeScope)
                        }
                    },
        )
    }

    private fun generateSubexpression(
        expr: Subexpression,
        bindings: TraversalBindings,
        context: TraversalContext,
    ): GeneratedExpression {
        val left = generate(expr.left, bindings, context)
        val right = generate(expr.right, listOf(TraversalBinding.Global(left.identifier, left.outputShape)), context)
        return GeneratedExpression(
            identifier = right.identifier,
            outputShape = right.outputShape,
            outputType = right.outputType,
            output = left.output + right.output,
        )
    }
}

internal fun generateCompare(
    safeNamer: SafeNamer,
    left: GeneratedExpression,
    right: GeneratedExpression,
    op: String,
): GeneratedExpression =
    if (left.outputType.isDoubleReference()) {
        generateCompare(safeNamer, left.dereference(safeNamer), right, op)
    } else if (right.outputType.isDoubleReference()) {
        generateCompare(safeNamer, left, right.dereference(safeNamer), op)
    } else {
        safeNamer.safeName("_cmp").let { ident ->
            return GeneratedExpression(
                identifier = ident,
                outputType = RustType.Bool,
                outputShape = TraversedShape.Bool(null),
                output =
                    if (left.isStringOrEnum() && right.isStringOrEnum()) {
                        writable {
                            val leftStr = left.convertToStrRef(safeNamer).also { it.output(this) }
                            val rightStr = right.convertToStrRef(safeNamer).also { it.output(this) }
                            rust("let $ident = ${leftStr.identifier} $op ${rightStr.identifier};")
                        }
                    } else if (left.isNumber() && right.isNumber()) {
                        writable {
                            val leftPrim =
                                left.convertToNumberPrimitive(safeNamer, left.outputType).also { it.output(this) }
                            val rightPrim =
                                right.convertToNumberPrimitive(safeNamer, left.outputType).also { it.output(this) }
                            rust("let $ident = ${leftPrim.identifier} $op ${rightPrim.identifier};")
                        }
                    } else if (left.isBool() && right.isBool()) {
                        writable {
                            val leftPrim = left.dereference(safeNamer).also { it.output(this) }
                            val rightPrim = right.dereference(safeNamer).also { it.output(this) }
                            rust("let $ident = ${leftPrim.identifier} $op ${rightPrim.identifier};")
                        }
                    } else {
                        throw UnsupportedJmesPathException("Comparison of ${left.outputType.render()} with ${right.outputType.render()} is not supported by smithy-rs")
                    },
            )
        }
    }

private fun renderMapToProject(
    writer: RustWriter,
    leftBinding: String,
    leftTargetSym: Any,
    right: GeneratedExpression,
) {
    writer.apply {
        Attribute.AllowClippyLetAndReturn.render(this)
        rustBlockTemplate(
            if (right.outputType is RustType.Option) {
                "fn map($leftBinding: &#{Left}) -> #{Right}"
            } else {
                "fn map($leftBinding: &#{Left}) -> #{Option}<#{Right}>"
            },
            *preludeScope,
            "Left" to leftTargetSym,
            "Right" to right.outputType,
        ) {
            right.output(this)
            if (right.outputType is RustType.Option) {
                rust(right.identifier)
            } else {
                rustTemplate("#{Some}(${right.identifier})", *preludeScope)
            }
        }
    }
}

/**
 * This function takes the `GeneratedExpression` of a projection expression's right-hand side (RHS)
 * and returns a pair:
 * - A `RustType` representing the final evaluation of the projection expression.
 * - A `Boolean` indicating whether the resulting vector needs to be flattened.
 *   Flattening ensures you get `Vec<&T>` instead of `Vec<&Vec<T>>`, which would otherwise cause
 *   subsequent projections to fail to compile.
 */
private fun projectionType(right: GeneratedExpression) =
    when {
        right.isArray() && right.outputType is RustType.Vec -> {
            // A case like `lists.structs[].[integer]` where RHS output type (`[integer]`) is `Vec<Option<&T>>`, and we want Vec<&T>
            RustType.Vec(right.outputType.member.stripOuter<RustType.Option>()) to true
        }

        right.isArray() && right.outputType is RustType.Option -> {
            // A case like `maps.structs[].strings` where RHS (strings) output type (`[strings]`) is `Option<&Vec<T>>`, and we want Vec<&T>
            RustType.Vec(
                right.outputType.member.stripOuter<RustType.Reference>().stripOuter<RustType.Vec>().asRef(),
            ) to true
        }

        else -> {
            RustType.Vec(right.outputType.stripOuter<RustType.Option>()) to false
        }
    }

private fun RustType.dereference(): RustType =
    if (this is RustType.Reference) {
        this.member.dereference()
    } else {
        this
    }

private fun RustType.isMap(): Boolean = this.dereference() is RustType.HashMap

private fun RustType.isString(): Boolean = this.dereference().let { it is RustType.String || it.isStr() }

private fun RustType.isStr(): Boolean = this.dereference().let { it is RustType.Opaque && it.name == "str" }

private fun RustType.isNumber(): Boolean = this.dereference().let { it is RustType.Integer || it is RustType.Float }

private fun RustType.isDoubleReference(): Boolean = this is RustType.Reference && this.member is RustType.Reference

private fun RustType.isCollectionOfOptions(): Boolean =
    try {
        collectionValue() is RustType.Option
    } catch (_: RuntimeException) {
        false
    }

private fun RustType.collectionValue(): RustType =
    when (this) {
        is RustType.Reference -> member.collectionValue()
        is RustType.Vec -> member
        is RustType.HashSet -> member
        is RustType.HashMap -> member
        else -> throw RuntimeException("expected collection type")
    }

private fun JmespathExpression.isLiteralNull(): Boolean = this == LiteralExpression.NULL
