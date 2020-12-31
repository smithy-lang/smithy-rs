/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.rustlang

import software.amazon.smithy.rust.codegen.smithy.RuntimeType

/**
 * A hierarchy of types handled by Smithy codegen
 */
sealed class RustType {

    // TODO: when Kotlin supports, sealed interfaces, seal Container
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
        override val namespace = "::std::string"
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
        // TODO: assert that underneath, the member is a String
        override val name: kotlin.String = "HashMap"
        override val namespace = "::std::collections"
    }

    data class HashSet(override val member: RustType) : RustType(), Container {
        // TODO: assert that underneath, the member is a String
        override val name: kotlin.String = SetType
        override val namespace = SetNamespace
    }

    data class Reference(val lifetime: kotlin.String?, override val member: RustType) : RustType(), Container {
        override val name: kotlin.String = member.name
    }

    data class Option(override val member: RustType) : RustType(), Container {
        override val name: kotlin.String = "Option"
        override val namespace = "::std::option"
    }

    data class Box(override val member: RustType) : RustType(), Container {
        override val name: kotlin.String = "Box"
        override val namespace = "::std::boxed"
    }

    data class Dyn(override val member: RustType) : RustType(), Container {
        override val name = "dyn"
        override val namespace: kotlin.String? = null
    }

    data class Vec(override val member: RustType) : RustType(), Container {
        override val name: kotlin.String = "Vec"
        override val namespace = "::std::vec"
    }

    data class Opaque(override val name: kotlin.String, override val namespace: kotlin.String? = null) : RustType()

    companion object {
        const val SetType = "BTreeSet"
        const val SetNamespace = "::std::collections"
    }
}

fun RustType.render(fullyQualified: Boolean): String {
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
 * Returns true if [this] contains [t] anywhere within it's tree. For example,
 * Option<Instant>.contains(Instant) would return true.
 * Option<Instant>.contains(Blob) would return false.
 */
fun <T : RustType> RustType.contains(t: T): Boolean {
    if (t == this) {
        return true
    }

    return when (this) {
        is RustType.Container -> this.member.contains(t)
        else -> false
    }
}

inline fun <reified T : RustType.Container> RustType.stripOuter(): RustType {
    return when (this) {
        is T -> this.member
        else -> this
    }
}

/**
 * Meta information about a Rust construction (field, struct, or enum)
 */
data class RustMetadata(
    val derives: Derives = Derives.Empty,
    val additionalAttributes: List<Attribute> = listOf(),
    val public: Boolean
) {
    fun withDerives(vararg newDerive: RuntimeType): RustMetadata =
        this.copy(derives = derives.copy(derives = derives.derives + newDerive))

    fun attributes(): List<Attribute> = additionalAttributes + derives
    fun renderAttributes(writer: RustWriter): RustMetadata {
        attributes().forEach {
            it.render(writer)
        }
        return this
    }

    fun renderVisibility(writer: RustWriter): RustMetadata {
        if (public) {
            writer.writeInline("pub ")
        }
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
    }
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

data class Custom(val annot: String, val symbols: List<RuntimeType> = listOf()) : Attribute() {
    override fun render(writer: RustWriter) {
        writer.raw("#[")
        writer.writeInline(annot)
        writer.write("]")

        symbols.forEach {
            writer.addDependency(it.dependency)
        }
    }
}
