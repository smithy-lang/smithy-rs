/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.client.smithy.generators.waiters

import software.amazon.smithy.jmespath.ExpressionSerializer
import software.amazon.smithy.jmespath.JmespathExpression
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
import software.amazon.smithy.model.shapes.CollectionShape
import software.amazon.smithy.model.shapes.ListShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.SafeNamer
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.asRef
import software.amazon.smithy.rust.codegen.core.rustlang.plus
import software.amazon.smithy.rust.codegen.core.rustlang.render
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.withBlock
import software.amazon.smithy.rust.codegen.core.rustlang.withBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.isOptional
import software.amazon.smithy.rust.codegen.core.smithy.rustType
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.orNull
import java.text.NumberFormat

/**
 * Contains information about the output of a visited [JmespathExpression].
 */
data class GeneratedExpression(
    /** The name of the identifier that this expression's evaluation is placed into */
    val identifier: String,
    /**
     * The Smithy shape for the output of the expression evaluation.
     *
     * This will be null for any computed values where there is no modeled shape
     * for the result of the evaluated expression. For example, the multi-select
     * list `['foo', 'baz']` is a list of string, but it isn't modeled anywhere,
     * so there is no Smithy shape to represent it.
     */
    val outputShape: Shape? = null,
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
    /** True if the type is a String, &str, or the shape is an enum shape. */
    fun isStringOrEnum(): Boolean = outputType.isString() || outputShape?.isEnumShape == true

    /** Dereferences this expression if it is a reference. */
    fun dereference(namer: SafeNamer): GeneratedExpression =
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
    fun convertToStrRef(namer: SafeNamer): GeneratedExpression =
        if (outputType is RustType.Reference && outputType.member is RustType.Reference) {
            dereference(namer).convertToStrRef(namer)
        } else if (!outputType.isString()) {
            namer.safeName("_tmp").let { tmp ->
                GeneratedExpression(
                    identifier = tmp,
                    outputType = RustType.String,
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
    fun convertToNumberPrimitive(
        namer: SafeNamer,
        desiredPrimitive: RustType,
    ): GeneratedExpression {
        check(outputType.isNumber() && desiredPrimitive.isNumber()) {
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
    abstract val shape: Shape

    /** Binds the given shape to the global namespace such that all its members are globally available */
    data class Global(
        override val rustName: String,
        override val shape: Shape,
    ) : TraversalBinding()

    /** Binds a shape to a name */
    data class Named(
        /** What this binding is referred to in JmesPath expressions */
        val jmespathName: String,
        override val rustName: String,
        override val shape: Shape,
    ) : TraversalBinding()
}

typealias TraversalBindings = List<TraversalBinding>

/**
 * Indicates a feature that's part of the JmesPath spec, but that we explicitly decided
 * not to support in smithy-rs due to the complexity of code generating it for Rust.
 */
data class UnsupportedJmesPathException(private val msg: String) : RuntimeException(msg)

/** Code can't be generated for the combination of the Smithy shape and the JmesPath expression. */
data class InvalidJmesPathTraversalException(private val msg: String) : RuntimeException(msg)

/** This indicates a bug in the code generator itself that should be fixed. */
data class JmesPathTraversalCodegenBugException(private val msg: String) : RuntimeException(msg)

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
 * - Functions `contains` and `length`. The `keys` function may be supported in the future.
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
    ): GeneratedExpression {
        try {
            val result =
                when (expr) {
                    is ComparatorExpression -> generateComparator(expr, bindings)
                    is FunctionExpression -> generateFunction(expr, bindings)
                    is FieldExpression -> generateField(expr, bindings)
                    is LiteralExpression -> generateLiteral(expr)
                    is MultiSelectListExpression -> generateMultiSelectList(expr, bindings)
                    is AndExpression -> generateAnd(expr, bindings)
                    is OrExpression -> generateOr(expr, bindings)
                    is NotExpression -> generateNot(expr, bindings)
                    is ObjectProjectionExpression -> generateObjectProjection(expr, bindings)
                    is FilterProjectionExpression -> generateFilterProjection(expr, bindings)
                    is ProjectionExpression -> generateProjection(expr, bindings)
                    is Subexpression -> generateSubexpression(expr, bindings)
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
            throw ex.copy(msg = "${ex.message}\nExpression: ${ExpressionSerializer().serialize(expr)}")
        } catch (ex: InvalidJmesPathTraversalException) {
            throw ex.copy(msg = "${ex.message}\nExpression: ${ExpressionSerializer().serialize(expr)}")
        } catch (ex: JmesPathTraversalCodegenBugException) {
            throw ex.copy(msg = "${ex.message}\nExpression: ${ExpressionSerializer().serialize(expr)}")
        }
    }

    private fun generateComparator(
        expr: ComparatorExpression,
        bindings: TraversalBindings,
    ): GeneratedExpression {
        val left = generate(expr.left, bindings)
        val right = generate(expr.right, bindings)
        return generateCompare(left, right, expr.comparator.toString())
    }

    private fun generateCompare(
        left: GeneratedExpression,
        right: GeneratedExpression,
        op: String,
    ): GeneratedExpression =
        if (left.outputType.isDoubleReference()) {
            generateCompare(left.dereference(safeNamer), right, op)
        } else if (right.outputType.isDoubleReference()) {
            generateCompare(left, right.dereference(safeNamer), op)
        } else {
            safeNamer.safeName("_cmp").let { ident ->
                return GeneratedExpression(
                    identifier = ident,
                    outputType = RustType.Bool,
                    output =
                        if (left.isStringOrEnum() && right.isStringOrEnum()) {
                            writable {
                                val leftStr = left.convertToStrRef(safeNamer).also { it.output(this) }
                                val rightStr = right.convertToStrRef(safeNamer).also { it.output(this) }
                                rust("let $ident = ${leftStr.identifier} $op ${rightStr.identifier};")
                            }
                        } else if (left.outputType.isNumber() && right.outputType.isNumber()) {
                            writable {
                                val leftPrim =
                                    left.convertToNumberPrimitive(safeNamer, left.outputType).also { it.output(this) }
                                val rightPrim =
                                    right.convertToNumberPrimitive(safeNamer, left.outputType).also { it.output(this) }
                                rust("let $ident = ${leftPrim.identifier} $op ${rightPrim.identifier};")
                            }
                        } else if (left.outputType.isBool() && right.outputType.isBool()) {
                            left.output + right.output +
                                writable {
                                    rust("let $ident = ${left.identifier} $op ${right.identifier};")
                                }
                        } else {
                            throw UnsupportedJmesPathException("Comparison of ${left.outputType.render()} with ${right.outputType.render()} is not supported by smithy-rs")
                        },
                )
            }
        }

    private fun generateFunction(
        expr: FunctionExpression,
        bindings: TraversalBindings,
    ): GeneratedExpression {
        val ident = safeNamer.safeName("_ret")
        return when (expr.name) {
            "length" -> {
                if (expr.arguments.size != 1) {
                    throw InvalidJmesPathTraversalException("Length function takes exactly one argument")
                }
                val arg = generate(expr.arguments[0], bindings)
                if (!arg.outputType.isCollection() && !arg.outputType.isString()) {
                    throw InvalidJmesPathTraversalException("Argument to `length` function must be a collection or string type")
                }
                GeneratedExpression(
                    identifier = ident,
                    outputType = RustType.Integer(64),
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
                val left = generate(expr.arguments[0], bindings)
                if (!left.outputType.isCollection() && !left.outputType.isString()) {
                    throw InvalidJmesPathTraversalException("First argument to `contains` function must be a collection or string type")
                }
                if (expr.arguments[1].isLiteralNull()) {
                    throw UnsupportedJmesPathException("Checking for null with `contains` is not supported in smithy-rs")
                }
                val right = generate(expr.arguments[1], bindings)
                if (!right.outputType.isNumber() && !right.outputType.isString() && right.outputShape?.isEnumShape != true) {
                    throw UnsupportedJmesPathException("Checking for anything other than numbers, strings, or enums in the `contains` function is not supported in smithy-rs")
                }
                if (left.outputType.isString()) {
                    return GeneratedExpression(
                        identifier = ident,
                        outputType = RustType.Bool,
                        output =
                            left.output + right.output +
                                writable {
                                    if (right.outputType.isString()) {
                                        rust("let $ident = ${left.identifier}.contains(${right.identifier});")
                                    } else {
                                        val tmp = safeNamer.safeName("_tmp")
                                        rust("let $tmp = ${right.identifier}.to_string();")
                                        rust("let $ident = ${left.identifier}.contains(&$tmp);")
                                    }
                                },
                    )
                } else {
                    return GeneratedExpression(
                        identifier = ident,
                        outputType = RustType.Bool,
                        output =
                            left.output + right.output +
                                writable {
                                    withBlockTemplate("let $ident = ${left.identifier}.iter().any(|_v| {", "});") {
                                        val compare =
                                            generateCompare(
                                                GeneratedExpression(
                                                    identifier = "_v",
                                                    outputShape =
                                                        (left.outputShape as? CollectionShape)?.member?.target?.let {
                                                            model.expectShape(
                                                                it,
                                                            )
                                                        },
                                                    outputType =
                                                        RustType.Reference(
                                                            lifetime = null,
                                                            member = left.outputType.collectionValue(),
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

            else -> throw UnsupportedJmesPathException("The `${expr.name}` function is not supported by smithy-rs")
        }
    }

    private fun generateField(
        expr: FieldExpression,
        bindings: TraversalBindings,
    ): GeneratedExpression {
        val globalBinding = bindings.find { it is TraversalBinding.Global }
        val namedBinding = bindings.find { it is TraversalBinding.Named && it.jmespathName == expr.name }
        if (namedBinding != null && namedBinding.shape is StructureShape) {
            // If there's a named binding that matches, then immediately return it
            return GeneratedExpression(
                identifier = namedBinding.rustName,
                outputShape = namedBinding.shape,
                outputType = symbolProvider.toSymbol(namedBinding.shape).rustType().asRef(),
                output = writable { },
            )
        } else if (globalBinding != null && globalBinding.shape is StructureShape) {
            // Otherwise, look in the global binding (if available)
            val member =
                globalBinding.shape.getMember(expr.name).orNull()
                    ?: throw InvalidJmesPathTraversalException("Member `${expr.name}` doesn't exist on ${globalBinding.shape.id}")
            val memberSym = symbolProvider.toSymbol(member)

            val target = model.expectShape(member.target)
            val targetSym = symbolProvider.toSymbol(target)

            val ident = safeNamer.safeName("_fld")
            return GeneratedExpression(
                identifier = ident,
                outputShape = target,
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
        } else if (namedBinding != null || globalBinding != null) {
            throw InvalidJmesPathTraversalException("Cannot look up fields in non-struct shapes")
        } else {
            throw JmesPathTraversalCodegenBugException("Missing jmespath traversal binding for ${expr.name}; available bindings: $bindings")
        }
    }

    private fun generateLiteral(expr: LiteralExpression): GeneratedExpression {
        val outputType =
            when (expr.value) {
                is Boolean -> RustType.Reference(lifetime = null, member = RustType.Bool)
                is Double -> RustType.Reference(lifetime = null, member = RustType.Float(64))
                is String -> RustType.Reference(lifetime = null, member = RustType.Opaque("str"))
                null -> throw UnsupportedJmesPathException("Literal nulls are not supported by smithy-rs")
                else -> throw UnsupportedJmesPathException("Literal expression '${ExpressionSerializer().serialize(expr)}' is not supported by smithy-rs")
            }

        fun fmtFloating(floating: Number) =
            NumberFormat.getInstance().apply { minimumFractionDigits = 1 }.format(floating)

        return safeNamer.safeName("_lit").uppercase().let { ident ->
            GeneratedExpression(
                identifier = ident,
                outputType = outputType,
                output =
                    writable {
                        when (val value = expr.value) {
                            is Boolean -> rust("const $ident: &bool = &$value;")
                            is Double -> {
                                rust("const $ident: #T = &${fmtFloating(value)};", outputType)
                            }

                            is String -> rust("const $ident: &str = ${value.dq()};")
                            else -> throw RuntimeException("unreachable")
                        }
                    },
            )
        }
    }

    private fun generateMultiSelectList(
        expr: MultiSelectListExpression,
        bindings: TraversalBindings,
    ): GeneratedExpression {
        val expressions =
            expr.expressions.map { subexpr ->
                generate(subexpr, bindings)
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
    ): GeneratedExpression = generateBooleanOp(expr, "&&", bindings)

    private fun generateOr(
        expr: OrExpression,
        bindings: TraversalBindings,
    ): GeneratedExpression = generateBooleanOp(expr, "||", bindings)

    private fun generateBooleanOp(
        expr: BinaryExpression,
        op: String,
        bindings: TraversalBindings,
    ): GeneratedExpression {
        val left = generate(expr.left, bindings)
        val right = generate(expr.right, bindings)
        if (!left.outputType.isBool() || !right.outputType.isBool()) {
            throw UnsupportedJmesPathException("Applying the `$op` operation doesn't support non-boolean types in smithy-rs")
        }

        return safeNamer.safeName("_bo").let { ident ->
            GeneratedExpression(
                identifier = ident,
                outputType = RustType.Bool,
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
    ): GeneratedExpression {
        val inner = generate(expr.expression, bindings)
        if (!inner.outputType.isBool()) {
            throw UnsupportedJmesPathException("Negation of a non-boolean type is not supported by smithy-rs")
        }

        return safeNamer.safeName("_not").let { ident ->
            GeneratedExpression(
                identifier = ident,
                outputType = RustType.Bool,
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
    ): GeneratedExpression {
        val maybeFlatten = expr.left
        if (maybeFlatten is SliceExpression) {
            throw UnsupportedJmesPathException("Slice expressions are not supported by smithy-rs")
        }
        if (maybeFlatten !is FlattenExpression) {
            throw UnsupportedJmesPathException("Only projection expressions with flattens are supported by smithy-rs")
        }
        val left = generate(maybeFlatten.expression, bindings)
        val leftTarget =
            when (val outputShape = left.outputShape) {
                is ListShape -> model.expectShape(outputShape.member.target)
                else -> throw InvalidJmesPathTraversalException("Left side of the flatten projection MUST resolve to a list or set shape")
            }
        val leftTargetSym = symbolProvider.toSymbol(leftTarget)

        // Short-circuit in the case where the projection is unnecessary
        if (left.outputType.isCollection() && expr.right is CurrentExpression) {
            return left
        }

        val right = generate(expr.right, listOf(TraversalBinding.Global("v", leftTarget)))
        val projectionType = RustType.Vec(right.outputType.asRef())

        return safeNamer.safeName("_prj").let { ident ->
            GeneratedExpression(
                identifier = ident,
                outputShape = right.outputShape,
                outputType = projectionType,
                output =
                    left.output +
                        writable {
                            rustBlock("let $ident = ${left.identifier}.iter().flat_map(") {
                                rustBlockTemplate(
                                    "fn map(v: &#{Left}) -> #{Option}<#{Right}>",
                                    *preludeScope,
                                    "Left" to leftTargetSym,
                                    "Right" to right.outputType,
                                ) {
                                    right.output(this)
                                    rustTemplate("#{Some}(${right.identifier})", *preludeScope)
                                }
                                rust("map")
                            }
                            rustTemplate(").collect::<#{Vec}<_>>();", *preludeScope)
                        },
            )
        }
    }

    private fun generateFilterProjection(
        expr: FilterProjectionExpression,
        bindings: TraversalBindings,
    ): GeneratedExpression {
        val left = generate(expr.left, bindings)
        if (!left.outputType.isList() && !left.outputType.isSet()) {
            throw UnsupportedJmesPathException("Filter projections can only be done on lists or sets in smithy-rs")
        }

        val leftTarget = model.expectShape((left.outputShape as ListShape).member.target)
        val leftTargetSym = symbolProvider.toSymbol(leftTarget)

        val right =
            if (expr.right is CurrentExpression) {
                left.copy(
                    outputType = left.outputType.collectionValue().asRef(),
                    output = writable {},
                )
            } else {
                generate(expr.right, listOf(TraversalBinding.Global("_v", leftTarget)))
            }

        val comparison = generate(expr.comparison, listOf(TraversalBinding.Global("_v", leftTarget)))
        if (!comparison.outputType.isBool()) {
            throw InvalidJmesPathTraversalException("The filter expression comparison must result in a boolean")
        }

        return safeNamer.safeName("_fprj").let { ident ->
            GeneratedExpression(
                identifier = ident,
                outputShape = null,
                outputType = RustType.Vec(right.outputType),
                output =
                    left.output +
                        writable {
                            rust("let $ident = ${left.identifier}.iter()")
                            withBlock(".filter({", "})") {
                                rustBlockTemplate("fn filter(_v: &#{Arg}) -> #{Option}<bool>", "Arg" to leftTargetSym, *preludeScope) {
                                    val toBool = comparison.dereference(safeNamer).also { it.output(this) }
                                    rustTemplate("#{Some}(${toBool.identifier})", *preludeScope)
                                }
                                rust("|v| filter(v).unwrap_or_default()")
                            }
                            if (expr.right !is CurrentExpression) {
                                withBlock(".flat_map({", "})") {
                                    rustBlockTemplate(
                                        "fn map(_v: &#{Left}) -> #{Option}<#{Right}>",
                                        *preludeScope,
                                        "Left" to leftTargetSym,
                                        "Right" to right.outputType,
                                    ) {
                                        right.output(this)
                                        rustTemplate("#{Some}(${right.identifier})", *preludeScope)
                                    }
                                    rust("map")
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
    ): GeneratedExpression {
        if (expr.left is CurrentExpression) {
            throw UnsupportedJmesPathException("Object projection cannot be done on computed maps in smithy-rs")
        }
        val left = generate(expr.left, bindings)
        if (!left.outputType.isMap()) {
            throw UnsupportedJmesPathException("Object projection is only supported on map types in smithy-rs")
        }
        if (left.outputShape == null) {
            throw UnsupportedJmesPathException("Object projection cannot be done on computed maps in smithy-rs")
        }

        val leftTarget = model.expectShape((left.outputShape as MapShape).value.target)
        val leftTargetSym = symbolProvider.toSymbol(leftTarget)

        val right =
            if (expr.right is CurrentExpression) {
                left.copy(
                    outputType = left.outputType.collectionValue().asRef(),
                    output = writable {},
                )
            } else {
                generate(expr.right, listOf(TraversalBinding.Global("_v", leftTarget)))
            }

        val ident = safeNamer.safeName("_oprj")
        return GeneratedExpression(
            identifier = ident,
            outputShape = null,
            outputType = RustType.Vec(right.outputType),
            output =
                left.output +
                    writable {
                        if (expr.right is CurrentExpression) {
                            rustTemplate("let $ident = ${left.identifier}.values().collect::<#{Vec}<_>>();", *preludeScope)
                        } else {
                            rustBlock("let $ident = ${left.identifier}.values().flat_map(") {
                                rustBlockTemplate(
                                    "fn map(_v: &#{Left}) -> #{Option}<#{Right}>",
                                    *preludeScope,
                                    "Left" to leftTargetSym,
                                    "Right" to right.outputType,
                                ) {
                                    right.output(this)
                                    rustTemplate("#{Some}(${right.identifier})", *preludeScope)
                                }
                                rust("map")
                            }
                            rustTemplate(").collect::<#{Vec}<_>>();", *preludeScope)
                        }
                    },
        )
    }

    private fun generateSubexpression(
        expr: Subexpression,
        bindings: TraversalBindings,
    ): GeneratedExpression {
        val left = generate(expr.left, bindings)
        val right = generate(expr.right, listOf(TraversalBinding.Global(left.identifier, left.outputShape!!)))
        return GeneratedExpression(
            identifier = right.identifier,
            outputShape = right.outputShape,
            outputType = right.outputType,
            output = left.output + right.output,
        )
    }
}

private fun RustType.dereference(): RustType =
    if (this is RustType.Reference) {
        this.member.dereference()
    } else {
        this
    }

private fun RustType.isList(): Boolean = this.dereference() is RustType.Vec

private fun RustType.isSet(): Boolean = this.dereference() is RustType.HashSet

private fun RustType.isMap(): Boolean = this.dereference() is RustType.HashMap

private fun RustType.isCollection(): Boolean = isList() || isSet() || isMap()

private fun RustType.isString(): Boolean = this.dereference().let { it is RustType.String || it.isStr() }

private fun RustType.isStr(): Boolean = this.dereference().let { it is RustType.Opaque && it.name == "str" }

private fun RustType.isNumber(): Boolean = this.dereference().let { it is RustType.Integer || it is RustType.Float }

private fun RustType.isBool(): Boolean = this.dereference() is RustType.Bool

private fun RustType.isDoubleReference(): Boolean = this is RustType.Reference && this.member is RustType.Reference

private fun RustType.collectionValue(): RustType =
    when (this) {
        is RustType.Reference -> member.collectionValue()
        is RustType.Vec -> member
        is RustType.HashSet -> member
        is RustType.HashMap -> member
        else -> throw RuntimeException("expected collection type")
    }

private fun JmespathExpression.isLiteralNull(): Boolean = this == LiteralExpression.NULL
