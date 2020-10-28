/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.codegen.core.SymbolProvider
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.BottomUpIndex
import software.amazon.smithy.model.knowledge.NullableIndex
import software.amazon.smithy.model.shapes.BigDecimalShape
import software.amazon.smithy.model.shapes.BigIntegerShape
import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.shapes.BooleanShape
import software.amazon.smithy.model.shapes.ByteShape
import software.amazon.smithy.model.shapes.DocumentShape
import software.amazon.smithy.model.shapes.DoubleShape
import software.amazon.smithy.model.shapes.FloatShape
import software.amazon.smithy.model.shapes.IntegerShape
import software.amazon.smithy.model.shapes.ListShape
import software.amazon.smithy.model.shapes.LongShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ResourceShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.SetShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeVisitor
import software.amazon.smithy.model.shapes.ShortShape
import software.amazon.smithy.model.shapes.SimpleShape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.TimestampShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.model.traits.HttpLabelTrait
import software.amazon.smithy.rust.codegen.lang.RustType
import software.amazon.smithy.rust.codegen.smithy.generators.toSnakeCase
import software.amazon.smithy.rust.codegen.smithy.traits.SyntheticInput
import software.amazon.smithy.utils.StringUtils

// TODO: currently, respecting integer types.
// Should we not? [Go does not]
val SimpleShapes = mapOf(
    BooleanShape::class to RustType.Bool,
    FloatShape::class to RustType.Float(32),
    DoubleShape::class to RustType.Float(64),
    ByteShape::class to RustType.Integer(8),
    ShortShape::class to RustType.Integer(16),
    IntegerShape::class to RustType.Integer(32),
    LongShape::class to RustType.Integer(64),
    StringShape::class to RustType.String
)

// TODO:
// Unions
// Recursive shapes
// Synthetics (blobs, timestamps)
// Operation
// Resources (do we do anything for resources?)
// Services
// Higher-level: Set, List, Map

fun Symbol.referenceClosure(): List<Symbol> {
    val referencedSymbols = this.references.map { it.symbol }
    return listOf(this) + referencedSymbols.flatMap { it.referenceClosure() }
}

data class SymbolVisitorConfig(val runtimeConfig: RuntimeConfig, val handleOptionality: Boolean = true, val handleRustBoxing: Boolean = true)

// TODO: consider if this is better handled as a wrapper
val DefaultConfig = SymbolVisitorConfig(runtimeConfig = RuntimeConfig(), handleOptionality = true, handleRustBoxing = true)

data class SymbolLocation(val filename: String, val namespace: String)

fun Symbol.Builder.locatedIn(symbolLocation: SymbolLocation): Symbol.Builder =
    this.definitionFile("src/${symbolLocation.filename}")
        .namespace("crate::${symbolLocation.namespace}", "::")

val Shapes = SymbolLocation("model.rs", "model")
val Errors = SymbolLocation("error.rs", "error")
val Operations = SymbolLocation("operation.rs", "operation")

class SymbolVisitor(
    private val model: Model,
    private val rootNamespace: String = "crate",
    private val config: SymbolVisitorConfig = DefaultConfig
) : SymbolProvider,
    ShapeVisitor<Symbol> {
    private val nullableIndex = NullableIndex(model)
    private val bottomUpIndex = BottomUpIndex(model)
    override fun toSymbol(shape: Shape): Symbol {
        return shape.accept(this)
    }

    override fun toMemberName(shape: MemberShape): String = shape.memberName.toSnakeCase()

    override fun blobShape(shape: BlobShape?): Symbol {
        return RuntimeType.Blob(config.runtimeConfig).toSymbol()
    }

    private fun handleOptionality(symbol: Symbol, member: MemberShape, container: Shape): Symbol {
        val httpLabeledInput = container.hasTrait(SyntheticInput::class.java) && member.hasTrait(HttpLabelTrait::class.java)
        return if (nullableIndex.isNullable(member) && !httpLabeledInput) {
            val builder = Symbol.builder()
            val rustType = RustType.Option(symbol.rustType())
            builder.rustType(rustType)
            builder.addReference(symbol)
            builder.name(rustType.name)
            builder.putProperty(SHAPE_KEY, member)
            builder.build()
        } else symbol
    }

    private fun handleRustBoxing(symbol: Symbol, shape: Shape): Symbol {
        return if (shape.hasTrait(RustBox::class.java)) {
            val builder = Symbol.builder()
            val rustType = RustType.Box(symbol.rustType())
            builder.rustType(rustType)
            builder.addReference(symbol)
            builder.name(rustType.name)
            builder.build()
        } else symbol
    }

    private fun simpleShape(shape: SimpleShape): Symbol {
        return symbolBuilder(shape, SimpleShapes.getValue(shape::class)).build()
    }

    override fun booleanShape(shape: BooleanShape): Symbol = simpleShape(shape)
    override fun byteShape(shape: ByteShape): Symbol = simpleShape(shape)
    override fun shortShape(shape: ShortShape): Symbol = simpleShape(shape)
    override fun integerShape(shape: IntegerShape): Symbol = simpleShape(shape)
    override fun longShape(shape: LongShape): Symbol = simpleShape(shape)
    override fun floatShape(shape: FloatShape): Symbol = simpleShape(shape)
    override fun doubleShape(shape: DoubleShape): Symbol = simpleShape(shape)
    override fun stringShape(shape: StringShape): Symbol {
        return if (shape.hasTrait(EnumTrait::class.java)) {
            symbolBuilder(shape, RustType.Opaque(shape.id.name)).locatedIn(Shapes).build()
        } else {
            simpleShape(shape)
        }
    }

    override fun listShape(shape: ListShape): Symbol {
        val inner = this.toSymbol(shape.member)
        return symbolBuilder(shape, RustType.Vec(inner.rustType())).addReference(inner).build()
    }

    override fun setShape(shape: SetShape): Symbol {
        val inner = this.toSymbol(shape.member)
        val builder = if (model.expectShape(shape.member.target).isStringShape) {
            // TODO: refactor / figure out how we want to handle prebaked symbols
            symbolBuilder(shape, RustType.HashSet(inner.rustType())).namespace(RuntimeType.HashSet.namespace, "::")
        } else {
            // only strings get put into actual sets because floats are unhashable
            symbolBuilder(shape, RustType.Vec(inner.rustType()))
        }
        return builder.addReference(inner).build()
    }

    override fun mapShape(shape: MapShape): Symbol {
        assert(shape.key.isStringShape)
        val key = this.toSymbol(shape.key)
        val value = this.toSymbol(shape.value)
        return symbolBuilder(shape, RustType.HashMap(key.rustType(), value.rustType())).namespace(
            "std::collections",
            "::"
        ).addReference(key).addReference(value).build()
    }

    override fun documentShape(shape: DocumentShape?): Symbol {
        TODO("Not yet implemented")
    }

    override fun bigIntegerShape(shape: BigIntegerShape?): Symbol {
        TODO("Not yet implemented")
    }

    override fun bigDecimalShape(shape: BigDecimalShape?): Symbol {
        TODO("Not yet implemented")
    }

    override fun operationShape(shape: OperationShape): Symbol {
        return symbolBuilder(shape, RustType.Opaque(shape.id.name.capitalize())).locatedIn(Operations).build()
    }

    override fun resourceShape(shape: ResourceShape?): Symbol {
        TODO("Not yet implemented")
    }

    override fun serviceShape(shape: ServiceShape?): Symbol {
        TODO("Not yet implemented")
    }

    override fun structureShape(shape: StructureShape): Symbol {
        val isError = shape.hasTrait(ErrorTrait::class.java)
        val isInput = shape.hasTrait(SyntheticInput::class.java)
        val name = StringUtils.capitalize(shape.id.name).letIf(isError) {
            // TODO: this is should probably be a configurable mixin
            it.replace("Exception", "Error")
        }
        val builder = symbolBuilder(shape, RustType.Opaque(name))
        return when {
            isError -> builder.locatedIn(Errors)
            // Input shapes live with their Operations
            isInput -> builder.locatedIn(Operations)
            else -> builder.locatedIn(Shapes)
        }.build()
    }

    override fun unionShape(shape: UnionShape): Symbol {
        val name = StringUtils.capitalize(shape.id.name)
        val builder = symbolBuilder(shape, RustType.Opaque(name)).locatedIn(Shapes)

        return builder.build()
    }

    override fun memberShape(shape: MemberShape): Symbol {
        val target = model.expectShape(shape.target)
        val targetSymbol = this.toSymbol(target)
        return targetSymbol.letIf(config.handleOptionality) {
            handleOptionality(it, shape, model.expectShape(shape.container))
        }.letIf(config.handleRustBoxing) {
            handleRustBoxing(it, shape)
        }
    }

    override fun timestampShape(shape: TimestampShape?): Symbol {
        return RuntimeType.Instant(config.runtimeConfig).toSymbol()
    }

    private fun symbolBuilder(shape: Shape?, rustType: RustType): Symbol.Builder {
        val builder = Symbol.builder().putProperty(SHAPE_KEY, shape)
        return builder.rustType(rustType)
            .name(rustType.name)
            // Every symbol that actually gets defined somewhere should set a definition file
            // If we ever generate a `thisisabug.rs`, we messed something up
            .definitionFile("thisisabug.rs")
    }
}

// TODO(chore): Move this to a useful place
private const val RUST_TYPE_KEY = "rusttype"
private const val SHAPE_KEY = "shape"

fun Symbol.Builder.rustType(rustType: RustType): Symbol.Builder {
    return this.putProperty(RUST_TYPE_KEY, rustType)
}

fun Symbol.rename(newName: String): Symbol {
    assert(this.rustType() is RustType.Opaque)
    return this.toBuilder().name(newName).rustType(RustType.Opaque(newName)).build()
}

fun Symbol.isOptional(): Boolean = when (this.rustType()) {
    is RustType.Option -> true
    else -> false
}

// Symbols should _always_ be created with a Rust type & shape attached
fun Symbol.rustType(): RustType = this.getProperty(RUST_TYPE_KEY, RustType::class.java).get()
fun Symbol.shape(): Shape = this.expectProperty(SHAPE_KEY, Shape::class.java)

fun <T> T.letIf(cond: Boolean, f: (T) -> T): T {
    return if (cond) {
        f(this)
    } else this
}
