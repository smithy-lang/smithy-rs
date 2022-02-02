/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy

import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.codegen.core.SymbolProvider
import software.amazon.smithy.model.Model
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
import software.amazon.smithy.model.traits.EnumDefinition
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.rust.codegen.rustlang.RustType
import software.amazon.smithy.rust.codegen.rustlang.stripOuter
import software.amazon.smithy.rust.codegen.smithy.traits.SyntheticInputTrait
import software.amazon.smithy.rust.codegen.smithy.traits.SyntheticOutputTrait
import software.amazon.smithy.rust.codegen.util.PANIC
import software.amazon.smithy.rust.codegen.util.hasTrait
import software.amazon.smithy.rust.codegen.util.orNull
import software.amazon.smithy.rust.codegen.util.toPascalCase
import software.amazon.smithy.rust.codegen.util.toSnakeCase
import kotlin.reflect.KClass

val SimpleShapes: Map<KClass<out Shape>, RustType> = mapOf(
    BooleanShape::class to RustType.Bool,
    FloatShape::class to RustType.Float(32),
    DoubleShape::class to RustType.Float(64),
    ByteShape::class to RustType.Integer(8),
    ShortShape::class to RustType.Integer(16),
    IntegerShape::class to RustType.Integer(32),
    LongShape::class to RustType.Integer(64),
    StringShape::class to RustType.String
)

data class SymbolVisitorConfig(
    val runtimeConfig: RuntimeConfig,
    val codegenConfig: CodegenConfig,
    val handleOptionality: Boolean = true,
    val handleRustBoxing: Boolean = true,
    val handleRequired: Boolean = false
)

val DefaultConfig =
    SymbolVisitorConfig(
        runtimeConfig = RuntimeConfig(),
        handleOptionality = true,
        handleRustBoxing = true,
        handleRequired = false,
        codegenConfig = CodegenConfig()
    )

data class SymbolLocation(val namespace: String) {
    val filename = "$namespace.rs"
}

val Models = SymbolLocation("model")
val Errors = SymbolLocation("error")
val Operations = SymbolLocation("operation")
val Serializers = SymbolLocation("serializer")
val Inputs = SymbolLocation("input")
val Outputs = SymbolLocation("output")

fun Symbol.makeOptional(): Symbol {
    return if (isOptional()) {
        this
    } else {
        val rustType = RustType.Option(this.rustType())
        Symbol.builder().rustType(rustType)
            .rustType(rustType)
            .addReference(this)
            .name(rustType.name)
            .build()
    }
}

fun Symbol.mapRustType(f: (RustType) -> RustType): Symbol {
    val newType = f(this.rustType())
    return Symbol.builder().rustType(newType)
        .addReference(this)
        .name(newType.name)
        .build()
}

fun Symbol.makeRustBoxed(): Symbol {
    val symbol = this
    val rustType = RustType.Box(symbol.rustType())
    return with(Symbol.builder()) {
        rustType(rustType)
        addReference(symbol)
        name(rustType.name)
        build()
    }
}

fun Symbol.Builder.locatedIn(symbolLocation: SymbolLocation): Symbol.Builder {
    val currentRustType = this.build().rustType()
    check(currentRustType is RustType.Opaque) { "Only Opaque can have their namespace updated" }
    val newRustType = currentRustType.copy(namespace = "crate::${symbolLocation.namespace}")
    return this.definitionFile("src/${symbolLocation.filename}")
        .namespace("crate::${symbolLocation.namespace}", "::")
        .rustType(newRustType)
}

data class MaybeRenamed(val name: String, val renamedFrom: String?)

interface RustSymbolProvider : SymbolProvider {
    fun config(): SymbolVisitorConfig
    fun toEnumVariantName(definition: EnumDefinition): MaybeRenamed?
    fun isRequiredTraitHandled(member: MemberShape, useNullableIndex: Boolean = true): Boolean
}

class SymbolVisitor(
    private val model: Model,
    private val serviceShape: ServiceShape?,
    private val config: SymbolVisitorConfig = DefaultConfig
) : RustSymbolProvider,
    ShapeVisitor<Symbol> {
    private val nullableIndex = NullableIndex.of(model)
    override fun config(): SymbolVisitorConfig = config

    override fun toSymbol(shape: Shape): Symbol {
        return shape.accept(this)
    }

    /**
     * This method is used in various parts of the code generation to understand if a type should be required or not
     * in the context of making struct members mandatory when the `@required` trait is used on the corresponding
     * structure shape members.
     *
     * This behaviour is enabled only in the server codegen and configured when instantiating a new [SymbolVisitor].
     *
     * This method allows to disambiguate between the following scenarios:
     * 1) client codegen: since the client does not have [config.handleRequired] set to `true`, we always return `false` to prevent
     *    changes in the client codegen behavior.
     * 2) server codegen: since the server has [config.handleRequired] set to `true`, we check first if the member is marked as required,
     *    otherwise handle the nullability index or just return `false`.
     *
     * The nullabiltiy index check is guarded by [useNullableIndex] as in certain scenarios (i.e. during deserialization) we still want to
     * use `Option`s to avoid changing the deserialization engine and delegate the translation from the [Option] to the real type inside
     * the [BuilderGenerator].
     */
    override fun isRequiredTraitHandled(member: MemberShape, useNullableIndex: Boolean) =
        config.handleRequired && (member.isRequired || (useNullableIndex && !nullableIndex.isNullable(member)))

    private fun Shape.contextName(): String {
        return if (serviceShape != null) {
            id.getName(serviceShape)
        } else {
            id.name
        }
    }

    override fun toEnumVariantName(definition: EnumDefinition): MaybeRenamed? {
        val baseName = definition.name.orNull()?.toPascalCase() ?: return null
        return MaybeRenamed(baseName, null)
    }

    override fun toMemberName(shape: MemberShape): String = when (val container = model.expectShape(shape.container)) {
        is StructureShape -> shape.memberName.toSnakeCase()
        is UnionShape -> shape.memberName.toPascalCase()
        else -> error("unexpected container shape: $container")
    }

    override fun blobShape(shape: BlobShape?): Symbol {
        return RuntimeType.Blob(config.runtimeConfig).toSymbol()
    }

    private fun handleOptionality(symbol: Symbol, member: MemberShape): Symbol {
        return if (nullableIndex.isNullable(member)) {
            symbol.makeOptional()
        } else symbol
    }

    private fun handleRequiredTrait(symbol: Symbol, member: MemberShape): Symbol =
        if (member.isRequired) {
            symbol
        } else if (nullableIndex.isNullable(member)) {
            symbol.makeOptional()
        } else {
            symbol
        }


    private fun handleRustBoxing(symbol: Symbol, shape: Shape): Symbol {
        return if (shape.hasTrait<RustBoxTrait>()) {
            val rustType = RustType.Box(symbol.rustType())
            with(Symbol.builder()) {
                rustType(rustType)
                addReference(symbol)
                name(rustType.name)
                build()
            }
        } else symbol
    }

    private fun simpleShape(shape: SimpleShape): Symbol {
        return symbolBuilder(shape, SimpleShapes.getValue(shape::class)).setDefault(Default.RustDefault).build()
    }

    override fun booleanShape(shape: BooleanShape): Symbol = simpleShape(shape)
    override fun byteShape(shape: ByteShape): Symbol = simpleShape(shape)
    override fun shortShape(shape: ShortShape): Symbol = simpleShape(shape)
    override fun integerShape(shape: IntegerShape): Symbol = simpleShape(shape)
    override fun longShape(shape: LongShape): Symbol = simpleShape(shape)
    override fun floatShape(shape: FloatShape): Symbol = simpleShape(shape)
    override fun doubleShape(shape: DoubleShape): Symbol = simpleShape(shape)
    override fun stringShape(shape: StringShape): Symbol {
        return if (shape.hasTrait<EnumTrait>()) {
            symbolBuilder(shape, RustType.Opaque(shape.contextName().toPascalCase())).locatedIn(Models).build()
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
            symbolBuilder(shape, RustType.HashSet(inner.rustType()))
        } else {
            // only strings get put into actual sets because floats are unhashable
            symbolBuilder(shape, RustType.Vec(inner.rustType()))
        }
        return builder.addReference(inner).build()
    }

    override fun mapShape(shape: MapShape): Symbol {
        val target = model.expectShape(shape.key.target)
        require(target.isStringShape) { "unexpected key shape: ${shape.key}: $target [keys must be strings]" }
        val key = this.toSymbol(shape.key)
        val value = this.toSymbol(shape.value)
        return symbolBuilder(shape, RustType.HashMap(key.rustType(), value.rustType())).addReference(key)
            .addReference(value).build()
    }

    override fun documentShape(shape: DocumentShape?): Symbol {
        return RuntimeType.Document(config.runtimeConfig).toSymbol()
    }

    override fun bigIntegerShape(shape: BigIntegerShape?): Symbol {
        TODO("Not yet implemented: https://github.com/awslabs/smithy-rs/issues/312")
    }

    override fun bigDecimalShape(shape: BigDecimalShape?): Symbol {
        TODO("Not yet implemented: https://github.com/awslabs/smithy-rs/issues/312")
    }

    override fun operationShape(shape: OperationShape): Symbol {
        return symbolBuilder(shape, RustType.Opaque(shape.contextName().capitalize())).locatedIn(Operations).build()
    }

    override fun resourceShape(shape: ResourceShape?): Symbol {
        TODO("Not yet implemented: resources are not supported")
    }

    override fun serviceShape(shape: ServiceShape?): Symbol {
        PANIC("symbol visitor should not be invoked in service shapes")
    }

    override fun structureShape(shape: StructureShape): Symbol {
        val isError = shape.hasTrait<ErrorTrait>()
        val isInput = shape.hasTrait<SyntheticInputTrait>()
        val isOutput = shape.hasTrait<SyntheticOutputTrait>()
        val name = shape.contextName().toPascalCase().letIf(isError && config.codegenConfig.renameExceptions) {
            it.replace("Exception", "Error")
        }
        val builder = symbolBuilder(shape, RustType.Opaque(name))
        return when {
            isError -> builder.locatedIn(Errors)
            isInput -> builder.locatedIn(Inputs)
            isOutput -> builder.locatedIn(Outputs)
            else -> builder.locatedIn(Models)
        }.build()
    }

    override fun unionShape(shape: UnionShape): Symbol {
        val name = shape.contextName().toPascalCase()
        val builder = symbolBuilder(shape, RustType.Opaque(name)).locatedIn(Models)

        return builder.build()
    }

    override fun memberShape(shape: MemberShape): Symbol {
        val target = model.expectShape(shape.target)
        val targetSymbol = this.toSymbol(target)
        if (config.handleOptionality && config.handleRequired) {
            throw CodegenException("CodegenVisitor 'handleOptionality' and 'handleRequired' configuration options cannot be both true at the same time")
        }
        // Handle boxing first so we end up with Option<Box<_>>, not Box<Option<_>>
        return targetSymbol.letIf(config.handleRustBoxing) {
            handleRustBoxing(it, shape)
        }.letIf(config.handleOptionality) {
            handleOptionality(it, shape)
        }.letIf(config.handleRequired) {
            handleRequiredTrait(it, shape)
        }
    }

    override fun timestampShape(shape: TimestampShape?): Symbol {
        return RuntimeType.DateTime(config.runtimeConfig).toSymbol()
    }

    private fun symbolBuilder(shape: Shape?, rustType: RustType): Symbol.Builder {
        val builder = Symbol.builder().putProperty(SHAPE_KEY, shape)
        return builder.rustType(rustType)
            .name(rustType.name)
            // Every symbol that actually gets defined somewhere should set a definition file
            // If we ever generate a `thisisabug.rs`, there is a bug in our symbol generation
            .definitionFile("thisisabug.rs")
    }
}

// TODO(chore): Move this to a useful place
private const val RUST_TYPE_KEY = "rusttype"
private const val SHAPE_KEY = "shape"
private const val SYMBOL_DEFAULT = "symboldefault"
private const val RENAMED_FROM_KEY = "renamedfrom"

fun Symbol.Builder.rustType(rustType: RustType): Symbol.Builder {
    return this.putProperty(RUST_TYPE_KEY, rustType)
}

fun Symbol.Builder.renamedFrom(name: String): Symbol.Builder {
    return this.putProperty(RENAMED_FROM_KEY, name)
}

fun Symbol.renamedFrom(): String? = this.getProperty(RENAMED_FROM_KEY, String::class.java).orNull()

fun Symbol.defaultValue(): Default = this.getProperty(SYMBOL_DEFAULT, Default::class.java).orElse(Default.NoDefault)
fun Symbol.Builder.setDefault(default: Default): Symbol.Builder {
    return this.putProperty(SYMBOL_DEFAULT, default)
}

sealed class Default {
    /**
     * This symbol has no default value. If the symbol is not optional, this will be an error during builder construction
     */
    object NoDefault : Default()

    /**
     * This symbol should use the Rust `std::default::Default` when unset
     */
    object RustDefault : Default()
}

/**
 * True when it is valid to use the default/0 value for [this] symbol during construction.
 */
fun Symbol.canUseDefault(): Boolean = this.defaultValue() != Default.NoDefault

/**
 * True when [this] is will be represented by Option<T> in Rust
 */
fun Symbol.isOptional(): Boolean = when (this.rustType()) {
    is RustType.Option -> true
    else -> false
}

fun Symbol.isBoxed(): Boolean = rustType().stripOuter<RustType.Option>() is RustType.Box

// Symbols should _always_ be created with a Rust type & shape attached
fun Symbol.rustType(): RustType = this.getProperty(RUST_TYPE_KEY, RustType::class.java).get()
fun Symbol.shape(): Shape = this.expectProperty(SHAPE_KEY, Shape::class.java)

fun <T> T.letIf(cond: Boolean, f: (T) -> T): T {
    return if (cond) {
        f(this)
    } else this
}
