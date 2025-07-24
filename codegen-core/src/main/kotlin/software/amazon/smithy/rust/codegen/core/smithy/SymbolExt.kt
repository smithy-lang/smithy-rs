/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.stripOuter
import software.amazon.smithy.rust.codegen.core.util.orNull

/** Set the symbolLocation for this symbol builder */
fun Symbol.Builder.locatedIn(rustModule: RustModule.LeafModule): Symbol.Builder {
    val currentRustType = this.build().rustType()
    check(currentRustType is RustType.Opaque) {
        "Only `RustType.Opaque` can have its namespace updated. Received $currentRustType."
    }
    val newRustType = currentRustType.copy(namespace = rustModule.fullyQualifiedPath())
    return this.definitionFile(rustModule.definitionFile())
        .namespace(rustModule.fullyQualifiedPath(), "::")
        .rustType(newRustType)
        .module(rustModule)
}

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
 * WARNING: This function does not update any symbol references (e.g., `symbol.addReference()`) on the
 * returned symbol. You will have to add those yourself if your logic relies on them.
 **/
fun Symbol.mapRustType(f: (RustType) -> RustType): Symbol {
    val newType = f(this.rustType())
    return Symbol.builder().rustType(newType)
        .name(newType.name)
        .build()
}

/**
 * Type representing the default value for a given type (e.g. for Strings, this is `""`).
 */
sealed class Default {
    /**
     * This symbol has no default value. If the symbol is not optional, this will error during builder construction
     */
    object NoDefault : Default()

    /**
     * This symbol should use the Rust `std::default::Default` when unset
     */
    object RustDefault : Default()

    /**
     * This symbol has a custom default value different from `Default::default`
     */
    data class NonZeroDefault(val value: Node) : Default()
}

/**
 * Returns true when it's valid to use the default/0 value for [this] symbol during construction.
 */
fun Symbol.canUseDefault(): Boolean = this.defaultValue() != Default.NoDefault

/**
 * True when [this] is will be represented by Option<T> in Rust
 */
fun Symbol.isOptional(): Boolean =
    when (this.rustType()) {
        is RustType.Option -> true
        else -> false
    }

fun Symbol.isCacheable(): Boolean {
    val rustType = this.rustType().stripOuter<RustType.Option>()
    if (rustType is RustType.Application) {
        return rustType.type.name == "Cacheable"
    }
    return false
}

fun Symbol.isRustBoxed(): Boolean = rustType().stripOuter<RustType.Option>() is RustType.Box

private const val RUST_TYPE_KEY = "rusttype"
private const val SHAPE_KEY = "shape"
private const val RUST_MODULE_KEY = "rustmodule"
private const val RENAMED_FROM_KEY = "renamedfrom"
private const val SYMBOL_DEFAULT = "symboldefault"

// Symbols should _always_ be created with a Rust type & shape attached
fun Symbol.rustType(): RustType = this.expectProperty(RUST_TYPE_KEY, RustType::class.java)

fun Symbol.Builder.rustType(rustType: RustType): Symbol.Builder = this.putProperty(RUST_TYPE_KEY, rustType)

fun Symbol.shape(): Shape = this.expectProperty(SHAPE_KEY, Shape::class.java)

fun Symbol.Builder.shape(shape: Shape?): Symbol.Builder = this.putProperty(SHAPE_KEY, shape)

fun Symbol.module(): RustModule.LeafModule = this.expectProperty(RUST_MODULE_KEY, RustModule.LeafModule::class.java)

fun Symbol.Builder.module(module: RustModule.LeafModule): Symbol.Builder = this.putProperty(RUST_MODULE_KEY, module)

fun Symbol.renamedFrom(): String? = this.getProperty(RENAMED_FROM_KEY, String::class.java).orNull()

fun Symbol.Builder.renamedFrom(name: String): Symbol.Builder = this.putProperty(RENAMED_FROM_KEY, name)

fun Symbol.defaultValue(): Default = this.getProperty(SYMBOL_DEFAULT, Default::class.java).orElse(Default.NoDefault)

fun Symbol.Builder.setDefault(default: Default): Symbol.Builder = this.putProperty(SYMBOL_DEFAULT, default)
