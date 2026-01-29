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
import software.amazon.smithy.model.traits.DefaultTrait
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustReservedWords
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.Visibility
import software.amazon.smithy.rust.codegen.core.smithy.traits.RustBoxTrait
import software.amazon.smithy.rust.codegen.core.util.PANIC
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.letIf
import software.amazon.smithy.rust.codegen.core.util.orNull
import software.amazon.smithy.rust.codegen.core.util.toPascalCase
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase
import kotlin.reflect.KClass

/** Map from Smithy Shapes to Rust Types */
val SimpleShapes: Map<KClass<out Shape>, RustType> =
    mapOf(
        BooleanShape::class to RustType.Bool,
        FloatShape::class to RustType.Float(32),
        DoubleShape::class to RustType.Float(64),
        ByteShape::class to RustType.Integer(8),
        ShortShape::class to RustType.Integer(16),
        IntegerShape::class to RustType.Integer(32),
        IntEnumShape::class to RustType.Integer(32),
        LongShape::class to RustType.Integer(64),
        StringShape::class to RustType.String,
    )

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
 * Make the return [value] optional if the [member] symbol is as well optional.
 */
fun SymbolProvider.wrapOptional(
    member: MemberShape,
    value: String,
): String =
    value.letIf(toSymbol(member).isOptional()) {
        "Some($value)"
    }

/**
 * Make the return [value] optional if the [member] symbol is not optional.
 */
fun SymbolProvider.toOptional(
    member: MemberShape,
    value: String,
): String =
    value.letIf(!toSymbol(member).isOptional()) {
        "Some($value)"
    }

/**
 * Services can rename their contained shapes. See https://smithy.io/2.0/spec/service-types.html
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
    settings: CoreRustSettings,
    final override val model: Model,
    private val serviceShape: ServiceShape?,
    override val config: RustSymbolProviderConfig,
) : RustSymbolProvider, ShapeVisitor<Symbol> {
    override val moduleProviderContext = ModuleProviderContext(settings, model, serviceShape)
    private val nullableIndex = NullableIndex.of(model)

    override fun toSymbol(shape: Shape): Symbol {
        return shape.accept(this)
    }

    override fun symbolForOperationError(operation: OperationShape): Symbol =
        toSymbol(operation).let { symbol ->
            val module = moduleForOperationError(operation)
            module.toType().resolve("${symbol.name}Error").toSymbol().toBuilder().locatedIn(module).build()
        }

    override fun symbolForEventStreamError(eventStream: UnionShape): Symbol =
        toSymbol(eventStream).let { symbol ->
            val module = moduleForEventStreamError(eventStream)
            module.toType().resolve("${symbol.name}Error").toSymbol().toBuilder().locatedIn(module).build()
        }

    override fun symbolForBuilder(shape: Shape): Symbol =
        toSymbol(shape).let { symbol ->
            val module = moduleForBuilder(shape)
            module.toType().resolve(config.nameBuilderFor(symbol)).toSymbol().toBuilder().locatedIn(module).build()
        }

    override fun toMemberName(shape: MemberShape): String {
        val container = model.expectShape(shape.container)
        return when {
            container is StructureShape -> shape.memberName.toSnakeCase()
            container is UnionShape || container is EnumShape || container.hasTrait<EnumTrait>() -> shape.memberName.toPascalCase()
            else -> error("unexpected container shape: $container")
        }
    }

    override fun blobShape(shape: BlobShape?): Symbol {
        return RuntimeType.blob(config.runtimeConfig).toSymbol()
    }

    /**
     * Produce `Box<T>` when the shape has the `RustBoxTrait`
     */
    private fun handleRustBoxing(
        symbol: Symbol,
        shape: Shape,
    ): Symbol {
        return if (shape.hasTrait<RustBoxTrait>()) {
            val rustType = RustType.Box(symbol.rustType())
            with(Symbol.builder()) {
                rustType(rustType)
                addReference(symbol)
                name(rustType.name)
                build()
            }
        } else {
            symbol
        }
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

    override fun intEnumShape(shape: IntEnumShape): Symbol = simpleShape(shape)

    override fun stringShape(shape: StringShape): Symbol {
        return if (shape.hasTrait<EnumTrait>()) {
            val rustType = RustType.Opaque(shape.contextName(serviceShape).toPascalCase())
            symbolBuilder(shape, rustType).locatedIn(moduleForShape(shape)).build()
        } else {
            symbolBuilder(shape, RustType.String).build()
        }
    }

    override fun listShape(shape: ListShape): Symbol {
        val inner = this.toSymbol(shape.member)
        return symbolBuilder(shape, RustType.Vec(inner.rustType())).addReference(inner).build()
    }

    override fun setShape(shape: SetShape): Symbol {
        val inner = this.toSymbol(shape.member)
        val builder =
            if (model.expectShape(shape.member.target).isStringShape) {
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
        return RuntimeType.bigInteger(config.runtimeConfig).toSymbol()
    }

    override fun bigDecimalShape(shape: BigDecimalShape?): Symbol {
        return RuntimeType.bigDecimal(config.runtimeConfig).toSymbol()
    }

    override fun operationShape(shape: OperationShape): Symbol {
        return symbolBuilder(
            shape,
            RustType.Opaque(
                shape.contextName(serviceShape)
                    .replaceFirstChar { it.uppercase() },
            ),
        )
            .locatedIn(moduleForShape(shape))
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
        val name =
            shape.contextName(serviceShape).toPascalCase().letIf(isError && config.renameExceptions) {
                it.replace("Exception", "Error")
            }
        return symbolBuilder(shape, RustType.Opaque(name)).locatedIn(moduleForShape(shape)).build()
    }

    override fun unionShape(shape: UnionShape): Symbol {
        val name = shape.contextName(serviceShape).toPascalCase()
        return symbolBuilder(shape, RustType.Opaque(name)).locatedIn(moduleForShape(shape)).build()
    }

    override fun memberShape(shape: MemberShape): Symbol {
        val target = model.expectShape(shape.target)
        val defaultValue =
            shape.getMemberTrait(model, DefaultTrait::class.java).orNull()?.let { trait ->
                if (target.isDocumentShape || target.isTimestampShape) {
                    Default.NonZeroDefault(trait.toNode())
                } else {
                    when (val value = trait.toNode()) {
                        Node.from(""), Node.from(0), Node.from(false), Node.arrayNode(), Node.objectNode() -> Default.RustDefault
                        Node.nullNode() -> Default.NoDefault
                        else -> Default.NonZeroDefault(value)
                    }
                }
            } ?: Default.NoDefault
        // Handle boxing first, so we end up with Option<Box<_>>, not Box<Option<_>>.
        return handleOptionality(
            handleRustBoxing(toSymbol(target), shape),
            shape,
            nullableIndex,
            config.nullabilityCheckMode,
        ).toBuilder().setDefault(defaultValue).build()
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
fun handleRustBoxing(
    symbol: Symbol,
    shape: MemberShape,
): Symbol =
    if (shape.hasTrait<RustBoxTrait>()) {
        symbol.makeRustBoxed()
    } else {
        symbol
    }

fun symbolBuilder(
    shape: Shape?,
    rustType: RustType,
): Symbol.Builder =
    Symbol.builder().shape(shape).rustType(rustType)
        .name(rustType.name)
        // Every symbol that actually gets defined somewhere should set a definition file
        // If we ever generate a `thisisabug.rs`, there is a bug in our symbol generation
        .definitionFile("thisisabug.rs")

fun handleOptionality(
    symbol: Symbol,
    member: MemberShape,
    nullableIndex: NullableIndex,
    nullabilityCheckMode: CheckMode,
): Symbol = symbol.letIf(nullableIndex.isMemberNullable(member, nullabilityCheckMode)) { symbol.makeOptional() }

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

/**
 *  You should rarely need this function, rust names in general should be symbol-aware,
 *  this is "automatic" if you use things like [software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate].
 */
fun String.unsafeToRustName(): String = RustReservedWords.escapeIfNeeded(this.toSnakeCase())
