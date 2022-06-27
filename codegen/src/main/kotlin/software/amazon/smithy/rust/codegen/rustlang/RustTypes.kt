/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.rustlang

import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.util.dq

/**
 * Dereference [input]
 *
 * Clippy is upset about `*&`, so if [input] is already referenced, simply strip the leading '&'
 */
fun autoDeref(input: String) = if (input.startsWith("&")) {
    input.removePrefix("&")
} else {
    "*$input"
}

/**
 * A hierarchy of types handled by Smithy codegen
 */
sealed class RustType {

    // TODO(kotlin): when Kotlin supports, sealed interfaces, seal Container
    /**
     * A Rust type that contains [member], another RustType. Used to generically operate over
     * shapes that contain other shapes, e.g. [stripOuter] and [contains].
     */
    interface Container {
        val member: RustType
        val namespace: kotlin.String?
        val name: kotlin.String
    }

    /*
     * Name refers to the top-level type for import purposes
     */
    abstract val name: kotlin.String

    open val namespace: kotlin.String? = null

    object Bool : RustType() {
        override val name: kotlin.String = "bool"
    }

    object String : RustType() {
        override val name: kotlin.String = "String"
        override val namespace = "std::string"
    }

    data class Float(val precision: Int) : RustType() {
        override val name: kotlin.String = "f$precision"
    }

    data class Integer(val precision: Int) : RustType() {
        override val name: kotlin.String = "i$precision"
    }

    data class Slice(override val member: RustType) : RustType(), Container {
        override val name: kotlin.String = ""
    }

    data class HashMap(val key: RustType, override val member: RustType) : RustType(), Container {
        // validating that `key` is a string occurs in the constructor in SymbolVisitor

        override val name: kotlin.String = "HashMap"
        override val namespace = "std::collections"

        companion object {
            val RuntimeType = RuntimeType("HashMap", dependency = null, namespace = "std::collections")
        }
    }

    data class HashSet(override val member: RustType) : RustType(), Container {
        override val name = Type
        override val namespace = Namespace

        companion object {
            // This is Vec intentionally. Note the following passage from the Smithy spec:
            //    Sets MUST be insertion ordered. Not all programming languages that support sets
            //    support ordered sets, requiring them may be overly burdensome for users, or conflict with language
            //    idioms. Such languages SHOULD store the values of sets in a list and rely on validation to ensure uniqueness.
            // It's possible that we could provide our own wrapper type in the future.
            const val Type = "Vec"
            const val Namespace = "std::vec"
            val RuntimeType = RuntimeType(name = Type, namespace = Namespace, dependency = null)
        }
    }

    data class Reference(val lifetime: kotlin.String?, override val member: RustType) : RustType(), Container {
        override val name = member.name
    }

    data class Option(override val member: RustType) : RustType(), Container {
        override val name = "Option"
        override val namespace = "std::option"

        /** Convert `Option<T>` to `Option<&T>` **/
        fun referenced(lifetime: kotlin.String?): Option {
            return Option(Reference(lifetime, this.member))
        }
    }

    data class Box(override val member: RustType) : RustType(), Container {
        override val name = "Box"
        override val namespace = "std::boxed"
    }

    data class Dyn(override val member: RustType) : RustType(), Container {
        override val name = "dyn"
        override val namespace: kotlin.String? = null
    }

    data class Vec(override val member: RustType) : RustType(), Container {
        override val name = "Vec"
        override val namespace = "std::vec"
    }

    data class Opaque(override val name: kotlin.String, override val namespace: kotlin.String? = null) : RustType()
}

/**
 * Return the fully qualified name of this type NOT including generic type parameters, references, etc.
 *
 * - To generate something like `std::collections::HashMap`, use this function.
 * - To generate something like `std::collections::HashMap<String, String>`, use [render]
 */
fun RustType.qualifiedName(): String {
    val namespace = this.namespace?.let { "$it::" } ?: ""
    return "$namespace$name"
}

/** Format this Rust type as an `impl Into<T>` */
fun RustType.implInto(fullyQualified: Boolean = true): String {
    return "impl Into<${this.render(fullyQualified)}>"
}

/** Format this Rust type so that it may be used as an argument type in a function definition */
fun RustType.asArgumentType(fullyQualified: Boolean = true): String {
    return when (this) {
        is RustType.String,
        is RustType.Box -> this.implInto(fullyQualified)
        else -> this.render(fullyQualified)
    }
}

/** Format this Rust type so that it may be used as an argument type in a function definition */
fun RustType.asArgumentValue(name: String) =
    when (this) {
        is RustType.String, is RustType.Box -> "$name.into()"
        else -> name
    }

/**
 * For a given name, generate an `Argument` data class containing pre-formatted strings for using this type when
 * writing a Rust function.
 */
fun RustType.asArgument(name: String) = Argument(
    "$name: ${this.asArgumentType()}",
    this.asArgumentValue(name),
    this.render(),
)

/**
 * Render this type, including references and generic parameters.
 * - To generate something like `std::collections::HashMap<String, String>`, use this function
 * - To generate something like `std::collections::HashMap`, use [qualifiedName]
 */
fun RustType.render(fullyQualified: Boolean = true): String {
    val namespace = if (fullyQualified) {
        this.namespace?.let { "$it::" } ?: ""
    } else ""
    val base = when (this) {
        is RustType.Bool -> this.name
        is RustType.Float -> this.name
        is RustType.Integer -> this.name
        is RustType.String -> this.name
        is RustType.Vec -> "${this.name}<${this.member.render(fullyQualified)}>"
        is RustType.Slice -> "[${this.member.render(fullyQualified)}]"
        is RustType.HashMap -> "${this.name}<${this.key.render(fullyQualified)}, ${this.member.render(fullyQualified)}>"
        is RustType.HashSet -> "${this.name}<${this.member.render(fullyQualified)}>"
        is RustType.Reference -> "&${this.lifetime?.let { "'$it" } ?: ""} ${this.member.render(fullyQualified)}"
        is RustType.Option -> "${this.name}<${this.member.render(fullyQualified)}>"
        is RustType.Box -> "${this.name}<${this.member.render(fullyQualified)}>"
        is RustType.Dyn -> "${this.name} ${this.member.render(fullyQualified)}"
        is RustType.Opaque -> this.name
    }
    return "$namespace$base"
}

/**
 * Returns true if [this] contains [t] anywhere within its tree. For example,
 * Option<DateTime>.contains(DateTime) would return true.
 * Option<DateTime>.contains(Blob) would return false.
 */
fun <T : RustType> RustType.contains(t: T): Boolean = when (this) {
    t -> true
    is RustType.Container -> this.member.contains(t)
    else -> false
}

inline fun <reified T : RustType.Container> RustType.stripOuter(): RustType = when (this) {
    is T -> this.member
    else -> this
}

/** Wraps a type in Option if it isn't already */
fun RustType.asOptional(): RustType = when (this) {
    is RustType.Option -> this
    else -> RustType.Option(this)
}

/**
 * Converts type to a reference
 *
 * For example:
 * - `String` -> `&String`
 * - `Option<T>` -> `Option<&T>`
 */
fun RustType.asRef(): RustType = when (this) {
    is RustType.Reference -> this
    is RustType.Option -> RustType.Option(member.asRef())
    else -> RustType.Reference(null, this)
}

/**
 * Converts type to its Deref target
 *
 * For example:
 * - `String` -> `str`
 * - `Option<String>` -> `Option<&str>`
 * - `Box<Something>` -> `&Something`
 */
fun RustType.asDeref(): RustType = when (this) {
    is RustType.Option -> if (member.isDeref()) {
        RustType.Option(member.asDeref().asRef())
    } else {
        this
    }
    is RustType.Box -> RustType.Reference(null, member)
    is RustType.String -> RustType.Opaque("str")
    is RustType.Vec -> RustType.Slice(member)
    else -> this
}

/** Returns true if the type implements Deref */
fun RustType.isDeref(): Boolean = when (this) {
    is RustType.Box -> true
    is RustType.String -> true
    is RustType.Vec -> true
    else -> false
}

/** Returns true if the type implements Copy */
fun RustType.isCopy(): Boolean = when (this) {
    is RustType.Float -> true
    is RustType.Integer -> true
    is RustType.Reference -> true
    is RustType.Bool -> true
    is RustType.Slice -> true
    is RustType.Option -> this.member.isCopy()
    else -> false
}

enum class Visibility {
    PRIVATE,
    PUBCRATE,
    PUBLIC
}

/**
 * Meta information about a Rust construction (field, struct, or enum).
 */
data class RustMetadata(
    val derives: Attribute.Derives = Attribute.Derives.Empty,
    val additionalAttributes: List<Attribute> = listOf(),
    val visibility: Visibility = Visibility.PRIVATE
) {
    fun withDerives(vararg newDerive: RuntimeType): RustMetadata =
        this.copy(derives = derives.copy(derives = derives.derives + newDerive))

    fun withoutDerives(vararg withoutDerives: RuntimeType) =
        this.copy(derives = derives.copy(derives = derives.derives - withoutDerives))

    private fun attributes(): List<Attribute> = additionalAttributes + derives

    fun renderAttributes(writer: RustWriter): RustMetadata {
        attributes().forEach {
            it.render(writer)
        }
        return this
    }

    private fun renderVisibility(writer: RustWriter): RustMetadata {
        writer.writeInline(
            when (visibility) {
                Visibility.PRIVATE -> ""
                Visibility.PUBCRATE -> "pub(crate) "
                Visibility.PUBLIC -> "pub "
            }
        )
        return this
    }

    fun render(writer: RustWriter) {
        renderAttributes(writer)
        renderVisibility(writer)
    }
}

/**
 * [Attributes](https://doc.rust-lang.org/reference/attributes.html) are general free form metadata
 * that are interpreted by the compiler.
 *
 * For example:
 * ```rust
 *
 * #[derive(Clone, PartialEq, Serialize)] // <-- this is an attribute
 * #[serde(serialize_with = "abc")] // <-- this is an attribute
 * struct Abc {
 *   a: i64
 * }
 */
sealed class Attribute {
    abstract fun render(writer: RustWriter)

    companion object {
        /**
         * [non_exhaustive](https://doc.rust-lang.org/reference/attributes/type_system.html#the-non_exhaustive-attribute)
         * indicates that more fields may be added in the future
         */
        val NonExhaustive = Custom("non_exhaustive")
        val AllowUnusedMut = Custom("allow(unused_mut)")
        val DocInline = Custom("doc(inline)")
    }

    data class Derives(val derives: Set<RuntimeType>) : Attribute() {
        override fun render(writer: RustWriter) {
            if (derives.isEmpty()) {
                return
            }
            writer.raw("#[derive(")
            derives.sortedBy { it.name }.forEach { derive ->
                writer.writeInline("#T, ", derive)
            }
            writer.write(")]")
        }

        companion object {
            val Empty = Derives(setOf())
        }
    }

    /**
     * A custom Attribute
     *
     * [annotation] represents the body of the attribute, e.g. `cfg(foo)` in `#[cfg(foo)]`
     * If [container] is set, this attribute refers to its container rather than its successor. This generates `#![cfg(foo)]`
     *
     * Finally, any symbols listed will be imported when this attribute is rendered. This enables using attributes like
     * `#[serde(Serialize)]` where `Serialize` is actually a symbol that must be imported.
     */
    data class Custom(
        val annotation: String,
        val symbols: List<RuntimeType> = listOf(),
        val container: Boolean = false
    ) : Attribute() {
        override fun render(writer: RustWriter) {
            val bang = if (container) "!" else ""
            writer.raw("#$bang[$annotation]")
            symbols.forEach {
                writer.addDependency(it.dependency)
            }
        }
    }

    data class Cfg(val cond: String) : Attribute() {
        override fun render(writer: RustWriter) {
            writer.raw("#[cfg($cond)]")
        }

        companion object {
            fun feature(feature: String) = Cfg("feature = ${feature.dq()}")
        }
    }
}

data class Argument(val argument: String, val value: String, val type: String)
