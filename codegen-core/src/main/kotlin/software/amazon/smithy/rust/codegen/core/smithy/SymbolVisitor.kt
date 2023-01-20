/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.codegen.core.SymbolProvider
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.NullableIndex
import software.amazon.smithy.model.knowledge.NullableIndex.CheckMode
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
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustReservedWords
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.Visibility
import software.amazon.smithy.rust.codegen.core.rustlang.stripOuter
import software.amazon.smithy.rust.codegen.core.smithy.traits.RustBoxTrait
import software.amazon.smithy.rust.codegen.core.smithy.traits.SyntheticInputTrait
import software.amazon.smithy.rust.codegen.core.smithy.traits.SyntheticOutputTrait
import software.amazon.smithy.rust.codegen.core.util.PANIC
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.letIf
import software.amazon.smithy.rust.codegen.core.util.orNull
import software.amazon.smithy.rust.codegen.core.util.toPascalCase
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase
import kotlin.reflect.KClass

/** Map from Smithy Shapes to Rust Types */
val SimpleShapes: Map<KClass<out Shape>, RustType> = mapOf(
    BooleanShape::class to RustType.Bool,
    FloatShape::class to RustType.Float(32),
    DoubleShape::class to RustType.Float(64),
    ByteShape::class to RustType.Integer(8),
    ShortShape::class to RustType.Integer(16),
    IntegerShape::class to RustType.Integer(32),
    LongShape::class to RustType.Integer(64),
    StringShape::class to RustType.String,
)

data class SymbolVisitorConfig(
    val runtimeConfig: RuntimeConfig,
    val renameExceptions: Boolean,
    val nullabilityCheckMode: CheckMode,
)

/**
 * Make the Rust type of a symbol optional (hold `Option<T>`)
 *
 * This is idempotent and will have no change if the type is already optional.
 */
fun Symbol.makeOptional(): Symbol =
    if (isOptional()) {
        this
    } else {
        val rustType = RustType.Option(this.rustType())
        Symbol.builder()
            .rustType(rustType)
            .addReference(this)
            .name(rustType.name)
            .build()
    }

/**
 * Make the Rust type of a symbol boxed (hold `Box<T>`).
 *
 * This is idempotent and will have no change if the type is already boxed.
 */
fun Symbol.makeRustBoxed(): Symbol =
    if (isRustBoxed()) {
        this
    } else {
        val rustType = RustType.Box(this.rustType())
        Symbol.builder()
            .rustType(rustType)
            .addReference(this)
            .name(rustType.name)
            .build()
    }

/**
 * Make the Rust type of a symbol wrapped in `MaybeConstrained`. (hold `MaybeConstrained<T>`).
 *
 * This is idempotent and will have no change if the type is already `MaybeConstrained<T>`.
 */
fun Symbol.makeMaybeConstrained(): Symbol =
    if (this.rustType() is RustType.MaybeConstrained) {
        this
    } else {
        val rustType = RustType.MaybeConstrained(this.rustType())
        Symbol.builder()
            .rustType(rustType)
            .addReference(this)
            .name(rustType.name)
            .build()
    }

/**
 * Map the [RustType] of a symbol with [f].
 *
 * WARNING: This function does not set any `SymbolReference`s on the returned symbol. You will have to add those
 * yourself if your logic relies on them.
 **/
fun Symbol.mapRustType(f: (RustType) -> RustType): Symbol {
    val newType = f(this.rustType())
    return Symbol.builder().rustType(newType)
        .name(newType.name)
        .build()
}

/** Set the symbolLocation for this symbol builder */
fun Symbol.Builder.locatedIn(rustModule: RustModule.LeafModule): Symbol.Builder {
    val currentRustType = this.build().rustType()
    check(currentRustType is RustType.Opaque) {
        "Only `Opaque` can have their namespace updated"
    }
    val newRustType = currentRustType.copy(namespace = rustModule.fullyQualifiedPath())
    return this.definitionFile(rustModule.definitionFile())
        .namespace(rustModule.fullyQualifiedPath(), "::")
        .rustType(newRustType)
        .module(rustModule)
}

/**
 * Track both the past and current name of a symbol
 *
 * When a symbol name conflicts with another name, we need to rename it. This tracks both names enabling us to generate helpful
 * docs that cover both cases.
 *
 * Note that this is only used for enum shapes an enum variant does not have its own symbol. For structures, the [Symbol.renamedFrom]
 * field will be set.
 */
data class MaybeRenamed(val name: String, val renamedFrom: String?)

/**
 * SymbolProvider interface that carries both the inner configuration and a function to produce an enum variant name.
 */
interface RustSymbolProvider : SymbolProvider {
    fun config(): SymbolVisitorConfig
    fun toEnumVariantName(definition: EnumDefinition): MaybeRenamed?
}

/**
 * Make the return [value] optional if the [member] symbol is as well optional.
 */
fun SymbolProvider.wrapOptional(member: MemberShape, value: String): String = value.letIf(toSymbol(member).isOptional()) { "Some($value)" }

/**
 * Make the return [value] optional if the [member] symbol is not optional.
 */
fun SymbolProvider.toOptional(member: MemberShape, value: String): String = value.letIf(!toSymbol(member).isOptional()) { "Some($value)" }

/**
 * Services can rename their contained shapes. See https://awslabs.github.io/smithy/1.0/spec/core/model.html#service
 * specifically, `rename`
 */
fun Shape.contextName(serviceShape: ServiceShape?): String {
    return if (serviceShape != null) {
        id.getName(serviceShape)
    } else {
        id.name
    }
}

/**
 * Base converter from `Shape` to `Symbol`. Shapes are the direct contents of the `Smithy` model. `Symbols` carry information
 * about Rust types, namespaces, dependencies, metadata as well as other information required to render a symbol.
 *
 * This is composed with other symbol visitors to handle behavior like Streaming shapes and determining the correct
 * derives for a given shape.
 */
open class SymbolVisitor(
    private val model: Model,
    private val serviceShape: ServiceShape?,
    private val config: SymbolVisitorConfig,
) : RustSymbolProvider,
    ShapeVisitor<Symbol> {
    private val nullableIndex = NullableIndex.of(model)
    override fun config(): SymbolVisitorConfig = config

    override fun toSymbol(shape: Shape): Symbol {
        return shape.accept(this)
    }

    /**
     * Return the name of a given `enum` variant. Note that this refers to `enum` in the Smithy context
     * where enum is a trait that can be applied to [StringShape] and not in the Rust context of an algebraic data type.
     *
     * Because enum variants are not member shape, a separate handler is required.
     */
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
        return RuntimeType.blob(config.runtimeConfig).toSymbol()
    }

    /**
     * Produce `Box<T>` when the shape has the `RustBoxTrait`
     */
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
            val rustType = RustType.Opaque(shape.contextName(serviceShape).toPascalCase())
            symbolBuilder(shape, rustType).locatedIn(ModelsModule).build()
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
        return RuntimeType.document(config.runtimeConfig).toSymbol()
    }

    override fun bigIntegerShape(shape: BigIntegerShape?): Symbol {
        TODO("Not yet implemented: https://github.com/awslabs/smithy-rs/issues/312")
    }

    override fun bigDecimalShape(shape: BigDecimalShape?): Symbol {
        TODO("Not yet implemented: https://github.com/awslabs/smithy-rs/issues/312")
    }

    override fun operationShape(shape: OperationShape): Symbol {
        return symbolBuilder(
            shape,
            RustType.Opaque(
                shape.contextName(serviceShape)
                    .replaceFirstChar { it.uppercase() },
            ),
        )
            .locatedIn(OperationsModule)
            .build()
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
        val name = shape.contextName(serviceShape).toPascalCase().letIf(isError && config.renameExceptions) {
            it.replace("Exception", "Error")
        }
        val builder = symbolBuilder(shape, RustType.Opaque(name))
        return when {
            isError -> builder.locatedIn(ErrorsModule)
            isInput -> builder.locatedIn(InputsModule)
            isOutput -> builder.locatedIn(OutputsModule)
            else -> builder.locatedIn(ModelsModule)
        }.build()
    }

    override fun unionShape(shape: UnionShape): Symbol {
        val name = shape.contextName(serviceShape).toPascalCase()
        val builder = symbolBuilder(shape, RustType.Opaque(name)).locatedIn(ModelsModule)

        return builder.build()
    }

    override fun memberShape(shape: MemberShape): Symbol {
        val target = model.expectShape(shape.target)
        // Handle boxing first, so we end up with Option<Box<_>>, not Box<Option<_>>.
        return handleOptionality(
            handleRustBoxing(toSymbol(target), shape),
            shape,
            nullableIndex,
            config.nullabilityCheckMode,
        )
    }

    override fun timestampShape(shape: TimestampShape?): Symbol {
        return RuntimeType.dateTime(config.runtimeConfig).toSymbol()
    }
}

/**
 * Boxes and returns [symbol], the symbol for the target of the member shape [shape], if [shape] is annotated with
 * [RustBoxTrait]; otherwise returns [symbol] unchanged.
 *
 * See `RecursiveShapeBoxer.kt` for the model transformation pass that annotates model shapes with [RustBoxTrait].
 */
fun handleRustBoxing(symbol: Symbol, shape: MemberShape): Symbol =
    if (shape.hasTrait<RustBoxTrait>()) {
        symbol.makeRustBoxed()
    } else symbol

fun symbolBuilder(shape: Shape?, rustType: RustType): Symbol.Builder {
    val builder = Symbol.builder().putProperty(SHAPE_KEY, shape)
    return builder.rustType(rustType)
        .name(rustType.name)
        // Every symbol that actually gets defined somewhere should set a definition file
        // If we ever generate a `thisisabug.rs`, there is a bug in our symbol generation
        .definitionFile("thisisabug.rs")
}

fun handleOptionality(symbol: Symbol, member: MemberShape, nullableIndex: NullableIndex, nullabilityCheckMode: CheckMode): Symbol =
    symbol.letIf(nullableIndex.isMemberNullable(member, nullabilityCheckMode)) { symbol.makeOptional() }

private const val RUST_TYPE_KEY = "rusttype"
private const val RUST_MODULE_KEY = "rustmodule"
private const val SHAPE_KEY = "shape"
private const val SYMBOL_DEFAULT = "symboldefault"
private const val RENAMED_FROM_KEY = "renamedfrom"

fun Symbol.Builder.rustType(rustType: RustType): Symbol.Builder = this.putProperty(RUST_TYPE_KEY, rustType)
fun Symbol.Builder.module(module: RustModule.LeafModule): Symbol.Builder = this.putProperty(RUST_MODULE_KEY, module)
fun Symbol.module(): RustModule.LeafModule = this.expectProperty(RUST_MODULE_KEY, RustModule.LeafModule::class.java)

/**
 * Creates a test module for this symbol.
 * For example if the symbol represents the name for the struct `struct MyStruct { ... }`,
 * this function will create the following inline module:
 * ```rust
 *  #[cfg(test)]
 *  mod test_my_struct { ... }
 * ```
 */
fun SymbolProvider.testModuleForShape(shape: Shape): RustModule.LeafModule {
    val symbol = toSymbol(shape)
    val rustName = symbol.name.unsafeToRustName()

    return RustModule.new(
        name = "test_$rustName",
        visibility = Visibility.PRIVATE,
        inline = true,
        parent = symbol.module(),
        additionalAttributes = listOf(Attribute.CfgTest),
    )
}

fun Symbol.Builder.renamedFrom(name: String): Symbol.Builder {
    return this.putProperty(RENAMED_FROM_KEY, name)
}

fun Symbol.renamedFrom(): String? = this.getProperty(RENAMED_FROM_KEY, String::class.java).orNull()

fun Symbol.defaultValue(): Default = this.getProperty(SYMBOL_DEFAULT, Default::class.java).orElse(Default.NoDefault)
fun Symbol.Builder.setDefault(default: Default): Symbol.Builder = this.putProperty(SYMBOL_DEFAULT, default)

/**
 * Type representing the default value for a given type. (eg. for Strings, this is `""`)
 */
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

fun Symbol.isRustBoxed(): Boolean = rustType().stripOuter<RustType.Option>() is RustType.Box

// Symbols should _always_ be created with a Rust type & shape attached
fun Symbol.rustType(): RustType = this.expectProperty(RUST_TYPE_KEY, RustType::class.java)
fun Symbol.shape(): Shape = this.expectProperty(SHAPE_KEY, Shape::class.java)

/**
 *  You should rarely need this function, rust names in general should be symbol-aware,
 *  this is "automatic" if you use things like [software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate].
 */
fun String.unsafeToRustName(): String = RustReservedWords.escapeIfNeeded(this.toSnakeCase())
