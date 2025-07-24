/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.rustlang

import software.amazon.smithy.rust.codegen.core.rustlang.Attribute.Companion.deprecated
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute.Companion.derive
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute.Companion.serde
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.util.dq

/**
 * Dereference [input]
 *
 * Clippy is upset about `*&`, so if [input] is already referenced, simply strip the leading '&'
 */
fun autoDeref(input: String) =
    if (input.startsWith("&")) {
        input.removePrefix("&")
    } else {
        "*$input"
    }

/**
 * A hierarchy of types handled by Smithy codegen
 */
sealed class RustType {
    /**
     * A Rust type that contains [member], another RustType. Used to generically operate over
     * shapes that contain other shapes, e.g. [stripOuter] and [contains].
     */
    sealed interface Container {
        val member: RustType
        val namespace: kotlin.String?
        val name: kotlin.String

        fun map(f: (RustType) -> RustType): RustType
    }

    /*
     * Name refers to the top-level type for import purposes
     */
    abstract val name: kotlin.String

    open val namespace: kotlin.String? = null

    /**
     * Get a writable for this `RustType`
     *
     * ```kotlin
     * // Declare a RustType
     * val t = RustType.Unit.writable
     * // Then, invoke the writable directly
     * t.invoke(writer)
     * // OR template it out
     *rustInlineTemplate("#{t:W}", "t" to t)
     * ```
     *
     * When formatted, the converted type will appear as such:
     *
     * | Type                                               | Formatted                                                            |
     * | -------------------------------------------------- | -------------------------------------------------------------------  |
     * | RustType.Unit                                      | ()                                                                   |
     * | RustType.Bool                                      | bool                                                                 |
     * | RustType.Float(32)                                 | f32                                                                  |
     * | RustType.Float(64)                                 | f64                                                                  |
     * | RustType.Integer(8)                                | i8                                                                   |
     * | RustType.Integer(16)                               | i16                                                                  |
     * | RustType.Integer(32)                               | i32                                                                  |
     * | RustType.Integer(64)                               | i64                                                                  |
     * | RustType.String                                    | std::string::String                                                  |
     * | RustType.Vec(RustType.String)                      | std::vec::Vec::<std::string::String>                                 |
     * | RustType.Slice(RustType.String)                    | [std::string::String]                                                |
     * | RustType.HashMap(RustType.String, RustType.String) | std::collections::HashMap::<std::string::String, std::string::String>|
     * | RustType.HashSet(RustType.String)                  | std::vec::Vec::<std::string::String>                                 |
     * | RustType.Reference("&", RustType.String)           | &std::string::String                                                 |
     * | RustType.Reference("&mut", RustType.String)        | &mut std::string::String                                             |
     * | RustType.Reference("&'static", RustType.String)    | &'static std::string::String                                         |
     * | RustType.Option(RustType.String)                   | std::option::Option<std::string::String>                             |
     * | RustType.Box(RustType.String)                      | std::boxed::Box<std::string::String>                                 |
     * | RustType.Opaque("SoCool", "zelda_is")              | zelda_is::SoCool                                                     |
     * | RustType.Opaque("SoCool")                          | SoCool                                                               |
     * | RustType.Dyn(RustType.Opaque("Foo", "foo"))        | dyn foo::Foo                                                         |
     */
    val writable = writable { rustInlineTemplate("#{this}", "this" to this@RustType) }

    object Unit : RustType() {
        override val name: kotlin.String = "()"
    }

    object Bool : RustType() {
        override val name: kotlin.String = "bool"
    }

    object String : RustType() {
        private val runtimeType = RuntimeType.String
        override val name = runtimeType.name
        override val namespace = runtimeType.namespace
    }

    data class Float(val precision: Int) : RustType() {
        override val name: kotlin.String = "f$precision"
    }

    data class Integer(val precision: Int) : RustType() {
        override val name: kotlin.String = "i$precision"
    }

    data class Slice(override val member: RustType) : RustType(), Container {
        override val name: kotlin.String = ""

        override fun map(f: (RustType) -> RustType): RustType = this.copy(f(member))
    }

    data class HashMap(val key: RustType, override val member: RustType) : RustType(), Container {
        // validating that `key` is a string occurs in the constructor in SymbolVisitor
        override val name = RuntimeType.HashMap.name

        override fun map(f: (RustType) -> RustType): RustType = this.copy(member = f(member))

        override val namespace = RuntimeType.HashMap.namespace

        companion object {
            val Type = RuntimeType.HashMap.name
            val Namespace = RuntimeType.HashMap.namespace
        }
    }

    data class HashSet(override val member: RustType) : RustType(), Container {
        override val name = RuntimeType.Vec.name
        override val namespace = RuntimeType.Vec.namespace

        override fun map(f: (RustType) -> RustType): RustType = this.copy(f(member))

        companion object {
            // This is Vec intentionally. Note the following passage from the Smithy spec:
            //    Sets MUST be insertion ordered. Not all programming languages that support sets
            //    support ordered sets, requiring them may be overly burdensome for users, or conflict with language
            //    idioms. Such languages SHOULD store the values of sets in a list and rely on validation to ensure uniqueness.
            // It's possible that we could provide our own wrapper type in the future.
            val Type = RuntimeType.Vec.name
            val Namespace = RuntimeType.Vec.namespace
        }
    }

    data class Reference(val lifetime: kotlin.String?, override val member: RustType) : RustType(), Container {
        override fun map(f: (RustType) -> RustType): RustType = this.copy(member = f(member))

        override val name = member.name
    }

    data class Option(override val member: RustType) : RustType(), Container {
        private val runtimeType = RuntimeType.Option
        override val name = runtimeType.name
        override val namespace = runtimeType.namespace

        override fun map(f: (RustType) -> RustType): RustType = this.copy(member = f(member))

        /** Convert `Option<T>` to `Option<&T>` **/
        fun referenced(lifetime: kotlin.String?): Option {
            return Option(Reference(lifetime, this.member))
        }
    }

    data class MaybeConstrained(override val member: RustType) : RustType(), Container {
        private val runtimeType = RuntimeType.MaybeConstrained
        override val name = runtimeType.name
        override val namespace = runtimeType.namespace

        override fun map(f: (RustType) -> RustType): RustType = this.copy(member = f(member))
    }

    data class Box(override val member: RustType) : RustType(), Container {
        private val runtimeType = RuntimeType.Box
        override val name = runtimeType.name
        override val namespace = runtimeType.namespace

        override fun map(f: (RustType) -> RustType): RustType = this.copy(member = f(member))
    }

    data class Dyn(override val member: RustType) : RustType(), Container {
        override val name = "dyn"
        override val namespace: kotlin.String? = null

        override fun map(f: (RustType) -> RustType): RustType = this.copy(member = f(member))
    }

    data class Vec(override val member: RustType) : RustType(), Container {
        private val runtimeType: RuntimeType = RuntimeType.Vec
        override val name = runtimeType.name
        override val namespace = runtimeType.namespace

        override fun map(f: (RustType) -> RustType): RustType = this.copy(member = f(member))
    }

    data class Opaque(override val name: kotlin.String, override val namespace: kotlin.String? = null) : RustType()

    /**
     * Represents application of a Rust type with the given arguments.
     *
     * For example, we can represent `HashMap<String, i64>` as
     * `RustType.Application(RustType.Opaque("HashMap"), listOf(RustType.String, RustType.Integer(64)))`.
     * This helps us to separate the type and the arguments which is useful in methods like [qualifiedName].
     */
    data class Application(val type: RustType, val args: List<RustType>) : RustType() {
        override val name = type.name
        override val namespace = type.namespace
    }
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
    return "impl ${RuntimeType.Into.render(fullyQualified)}<${this.render(fullyQualified)}>"
}

/** Format this Rust type so that it may be used as an argument type in a function definition */
fun RustType.asArgumentType(fullyQualified: Boolean = true): String {
    return when (this) {
        is RustType.String, is RustType.Box -> this.implInto(fullyQualified)
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
fun RustType.asArgument(name: String) =
    Argument(
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
    val namespace =
        if (fullyQualified) {
            this.namespace?.let { "$it::" } ?: ""
        } else {
            ""
        }
    val base =
        when (this) {
            is RustType.Unit -> this.name
            is RustType.Bool -> this.name
            is RustType.Float -> this.name
            is RustType.Integer -> this.name
            is RustType.String -> this.name
            is RustType.Vec -> "${this.name}::<${this.member.render(fullyQualified)}>"
            is RustType.Slice -> "[${this.member.render(fullyQualified)}]"
            is RustType.HashMap -> "${this.name}::<${this.key.render(fullyQualified)}, ${
                this.member.render(
                    fullyQualified,
                )
            }>"

            is RustType.HashSet -> "${this.name}::<${this.member.render(fullyQualified)}>"
            is RustType.Reference -> {
                if (this.lifetime == "&") {
                    "&${this.member.render(fullyQualified)}"
                } else {
                    "&${this.lifetime?.let { "'$it " } ?: ""}${this.member.render(fullyQualified)}"
                }
            }

            is RustType.Application -> {
                val args = this.args.joinToString(", ") { it.render(fullyQualified) }
                "${this.name}<$args>"
            }

            is RustType.Option -> "${this.name}<${this.member.render(fullyQualified)}>"
            is RustType.Box -> "${this.name}<${this.member.render(fullyQualified)}>"
            is RustType.Dyn -> "${this.name} ${this.member.render(fullyQualified)}"
            is RustType.Opaque -> this.name
            is RustType.MaybeConstrained -> "${this.name}<${this.member.render(fullyQualified)}>"
        }
    return "$namespace$base"
}

/**
 * Returns true if [this] contains [t] anywhere within its tree. For example,
 * Option<DateTime>.contains(DateTime) would return true.
 * Option<DateTime>.contains(Blob) would return false.
 */
fun <T : RustType> RustType.contains(t: T): Boolean =
    when (this) {
        t -> true
        is RustType.Container -> this.member.contains(t)
        else -> false
    }

inline fun <reified T : RustType.Container> RustType.stripOuter(): RustType =
    when (this) {
        is T -> this.member
        else -> this
    }

/** Extracts the inner Reference type */
fun RustType.innerReference(): RustType? =
    when (this) {
        is RustType.Reference -> this
        is RustType.Container -> this.member.innerReference()
        else -> null
    }

/** Wraps a type in Option if it isn't already */
fun RustType.asOptional(): RustType =
    when (this) {
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
fun RustType.asRef(): RustType =
    when (this) {
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
fun RustType.asDeref(): RustType =
    when (this) {
        is RustType.Option ->
            if (member.isDeref()) {
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
fun RustType.isDeref(): Boolean =
    when (this) {
        is RustType.Box -> true
        is RustType.String -> true
        is RustType.Vec -> true
        else -> false
    }

/** Returns true if the type implements Copy */
fun RustType.isCopy(): Boolean =
    when (this) {
        is RustType.Float -> true
        is RustType.Integer -> true
        is RustType.Reference -> true
        is RustType.Bool -> true
        is RustType.Slice -> true
        is RustType.Option -> this.member.isCopy()
        else -> false
    }

/** Returns true if the type implements Eq */
fun RustType.isEq(): Boolean =
    when (this) {
        is RustType.Integer -> true
        is RustType.Bool -> true
        is RustType.String -> true
        is RustType.Unit -> true
        is RustType.Container -> this.member.isEq()
        else -> false
    }

/** Recursively replaces lifetimes with the new lifetime */
fun RustType.replaceLifetimes(newLifetime: String?): RustType =
    when (this) {
        is RustType.Option -> copy(member = member.replaceLifetimes(newLifetime))
        is RustType.Vec -> copy(member = member.replaceLifetimes(newLifetime))
        is RustType.HashSet -> copy(member = member.replaceLifetimes(newLifetime))
        is RustType.HashMap ->
            copy(
                key = key.replaceLifetimes(newLifetime),
                member = member.replaceLifetimes(newLifetime),
            )

        is RustType.Reference -> copy(lifetime = newLifetime)
        else -> this
    }

enum class Visibility {
    PRIVATE,
    PUBCRATE,
    PUBLIC,
    ;

    companion object {
        fun publicIf(
            condition: Boolean,
            ifNot: Visibility,
        ): Visibility =
            if (condition) {
                PUBLIC
            } else {
                ifNot
            }
    }

    fun toRustQualifier(): String =
        when (this) {
            PRIVATE -> ""
            PUBCRATE -> "pub(crate)"
            PUBLIC -> "pub"
        }
}

/**
 * Meta information about a Rust construction (field, struct, or enum).
 */
data class RustMetadata(
    val derives: Set<RuntimeType> = setOf(),
    val additionalAttributes: List<Attribute> = listOf(),
    val visibility: Visibility = Visibility.PRIVATE,
) {
    fun withDerives(vararg newDerives: RuntimeType): RustMetadata = this.copy(derives = derives + newDerives)

    fun withoutDerives(vararg withoutDerives: RuntimeType) = this.copy(derives = derives - withoutDerives.toSet())

    fun renderAttributes(writer: RustWriter): RustMetadata {
        val (deriveHelperAttrs, otherAttrs) = additionalAttributes.partition { it.isDeriveHelper }
        otherAttrs.forEach {
            it.render(writer)
        }

        Attribute(derive(derives)).render(writer)

        // Derive helper attributes must come after derive, see https://github.com/rust-lang/rust/issues/79202
        deriveHelperAttrs.forEach {
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
            },
        )
        return this
    }

    fun render(writer: RustWriter) {
        renderAttributes(writer)
        renderVisibility(writer)
    }

    /**
     * If `true`, the Rust symbol that this metadata references derives a `Debug` implementation.
     * If `false`, then it doesn't.
     */
    fun hasDebugDerive(): Boolean {
        return derives.contains(RuntimeType.Debug)
    }
}

data class Argument(val argument: String, val value: String, val type: String)

/**
 * AttributeKind differentiates between the two kinds of attribute macros: inner and outer.
 * See the variant docs for more info, and the official Rust [Attribute Macro](https://doc.rust-lang.org/reference/attributes.html)
 * for even MORE info.
 */
enum class AttributeKind {
    /**
     * Inner attributes, written with a bang (!) after the hash (#), apply to the item that the attribute is declared within.
     */
    Inner,

    /**
     * Outer attributes, written without the bang after the hash, apply to the thing that follows the attribute.
     */
    Outer,
}

/**
 * [Attributes](https://doc.rust-lang.org/reference/attributes.html) are general free form metadata
 * that are interpreted by the compiler.
 *
 * If the attribute is a "derive helper", such as  `#[serde]`, set `isDeriveHelper` to `true` so it is sorted correctly after
 * the derive attribute is rendered. (See https://github.com/rust-lang/rust/issues/79202 for why sorting matters.)
 *
 * For example:
 * ```rust
 * #[allow(missing_docs)] // <-- this is an attribute, and it is not a derive helper
 * #[derive(Clone, PartialEq, Serialize)] // <-- this is an attribute
 * #[serde(serialize_with = "abc")] // <-- this attribute is a derive helper because the `Serialize` derive uses it
 * struct Abc {
 *   a: i64
 * }
 * ```
 */
class Attribute(val inner: Writable, val isDeriveHelper: Boolean = false) {
    constructor(str: String) : this(writable(str))
    constructor(str: String, isDeriveHelper: Boolean) : this(writable(str), isDeriveHelper)
    constructor(runtimeType: RuntimeType) : this(runtimeType.writable)

    fun render(
        writer: RustWriter,
        attributeKind: AttributeKind = AttributeKind.Outer,
    ) {
        // Writing "#[]" with nothing inside it is meaningless
        if (inner.isNotEmpty()) {
            when (attributeKind) {
                AttributeKind.Inner -> writer.rust("##![#W]", inner)
                AttributeKind.Outer -> writer.rust("##[#W]", inner)
            }
        }
    }

    // These were supposed to be a part of companion object but we decided to move it out to here to avoid NPE
    // You can find the discussion here.
    // https://github.com/smithy-lang/smithy-rs/discussions/2248
    fun serdeSerialize(): Attribute {
        return Attribute(
            cfgAttr(
                all(writable("aws_sdk_unstable"), feature("serde-serialize")),
                derive(RuntimeType.SerdeSerialize),
            ),
        )
    }

    fun serdeDeserialize(): Attribute {
        return Attribute(
            cfgAttr(
                all(writable("aws_sdk_unstable"), feature("serde-deserialize")),
                derive(RuntimeType.SerdeDeserialize),
            ),
        )
    }

    fun serdeSkip(): Attribute {
        return Attribute(
            cfgAttr(
                all(
                    writable("aws_sdk_unstable"),
                    any(feature("serde-serialize"), feature("serde-deserialize")),
                ),
                serde("skip"),
            ),
        )
    }

    fun serdeSerializeOrDeserialize(): Attribute {
        return Attribute(
            cfg(
                all(
                    writable("aws_sdk_unstable"),
                    any(feature("serde-serialize"), feature("serde-deserialize")),
                ),
            ),
        )
    }

    companion object {
        val AllowNeedlessQuestionMark = Attribute(allow("clippy::needless_question_mark"))
        val AllowClippyBoxedLocal = Attribute(allow("clippy::boxed_local"))
        val AllowClippyEmptyLineAfterDocComments = Attribute(allow("clippy::empty_line_after_doc_comments"))
        val AllowClippyLetAndReturn = Attribute(allow("clippy::let_and_return"))
        val AllowClippyNeedlessBorrow = Attribute(allow("clippy::needless_borrow"))
        val AllowClippyNeedlessLifetimes = Attribute(allow("clippy::needless_lifetimes"))
        val AllowClippyNewWithoutDefault = Attribute(allow("clippy::new_without_default"))
        val AllowClippyNonLocalDefinitions = Attribute(allow("clippy::non_local_definitions"))
        val AllowClippyUnnecessaryWraps = Attribute(allow("clippy::unnecessary_wraps"))
        val AllowClippyUselessConversion = Attribute(allow("clippy::useless_conversion"))
        val AllowClippyUnnecessaryLazyEvaluations = Attribute(allow("clippy::unnecessary_lazy_evaluations"))
        val AllowClippyTooManyArguments = Attribute(allow("clippy::too_many_arguments"))
        val AllowDeadCode = Attribute(allow("dead_code"))
        val AllowDeprecated = Attribute(allow("deprecated"))
        val AllowIrrefutableLetPatterns = Attribute(allow("irrefutable_let_patterns"))
        val AllowMissingDocs = Attribute(allow("missing_docs"))
        val AllowNonSnakeCase = Attribute(allow("non_snake_case"))
        val AllowUnreachableCode = Attribute(allow("unreachable_code"))
        val AllowUnreachablePatterns = Attribute(allow("unreachable_patterns"))
        val AllowUnused = Attribute(allow("unused"))
        val AllowUnusedImports = Attribute(allow("unused_imports"))
        val AllowUnusedMut = Attribute(allow("unused_mut"))
        val AllowUnusedVariables = Attribute(allow("unused_variables"))
        val CfgTest = Attribute(cfg("test"))
        val DenyDeprecated = Attribute(deny("deprecated"))
        val DenyMissingDocs = Attribute(deny("missing_docs"))
        val DocHidden = Attribute(doc("hidden"))
        val DocInline = Attribute(doc("inline"))
        val NoImplicitPrelude = Attribute("no_implicit_prelude")

        fun shouldPanic(expectedMessage: String? = null): Attribute =
            if (expectedMessage != null) {
                Attribute(macroWithArgs("should_panic", "expected = ${expectedMessage.dq()}"))
            } else {
                Attribute("should_panic")
            }

        val Test = Attribute("test")
        val TokioTest = Attribute(RuntimeType.Tokio.resolve("test").writable)
        val TracedTest = Attribute(RuntimeType.TracingTest.resolve("traced_test").writable)
        val AwsSdkUnstableAttribute = Attribute(cfg("aws_sdk_unstable"))

        /**
         * [non_exhaustive](https://doc.rust-lang.org/reference/attributes/type_system.html#the-non_exhaustive-attribute)
         * indicates that more fields may be added in the future
         */
        val NonExhaustive = Attribute("non_exhaustive")

        /**
         * Mark the following type as deprecated. If you know why and in what version something was deprecated, then
         * using [deprecated] is preferred.
         */
        val Deprecated = Attribute("deprecated")

        private fun macroWithArgs(
            name: String,
            vararg args: RustWriter.() -> Unit,
        ): Writable =
            {
                // Macros that require args can't be empty
                if (args.isNotEmpty()) {
                    rustInline("$name(#W)", args.toList().join(", "))
                }
            }

        private fun macroWithArgs(
            name: String,
            vararg args: String,
        ): Writable =
            {
                // Macros that require args can't be empty
                if (args.isNotEmpty()) {
                    rustInline("$name(${args.joinToString(", ")})")
                }
            }

        fun all(vararg attrMacros: Writable): Writable = macroWithArgs("all", *attrMacros)

        fun cfgAttr(vararg attrMacros: Writable): Writable = macroWithArgs("cfg_attr", *attrMacros)

        fun allow(lints: Collection<String>): Writable = macroWithArgs("allow", *lints.toTypedArray())

        fun allow(vararg lints: String): Writable = macroWithArgs("allow", *lints)

        fun deny(vararg lints: String): Writable = macroWithArgs("deny", *lints)

        fun serde(vararg lints: String): Writable = macroWithArgs("serde", *lints)

        fun any(vararg attrMacros: Writable): Writable = macroWithArgs("any", *attrMacros)

        fun cfg(vararg attrMacros: Writable): Writable = macroWithArgs("cfg", *attrMacros)

        fun cfg(vararg attrMacros: String): Writable = macroWithArgs("cfg", *attrMacros)

        fun doc(vararg attrMacros: Writable): Writable = macroWithArgs("doc", *attrMacros)

        fun doc(str: String): Writable = macroWithArgs("doc", writable(str))

        fun not(vararg attrMacros: Writable): Writable = macroWithArgs("not", *attrMacros)

        fun feature(feature: String) = writable("feature = ${feature.dq()}")

        fun featureGate(featureName: String): Attribute {
            return Attribute(cfg(feature(featureName)))
        }

        fun deprecated(
            since: String? = null,
            note: String? = null,
        ): Writable {
            val optionalFields = mutableListOf<Writable>()
            if (!note.isNullOrEmpty()) {
                optionalFields.add(pair("note" to note.dq()))
            }

            if (!since.isNullOrEmpty()) {
                optionalFields.add(pair("since" to since.dq()))
            }

            return {
                rustInline("deprecated")
                if (optionalFields.isNotEmpty()) {
                    rustInline("(#W)", optionalFields.join(", "))
                }
            }
        }

        fun derive(vararg runtimeTypes: RuntimeType): Writable =
            {
                // Empty derives are meaningless
                if (runtimeTypes.isNotEmpty()) {
                    // Sorted derives look nicer than unsorted, and it makes test output easier to predict
                    val writables = runtimeTypes.sortedBy { it.path }.map { it.writable }.join(", ")
                    rustInline("derive(#W)", writables)
                }
            }

        fun derive(runtimeTypes: Collection<RuntimeType>): Writable = derive(*runtimeTypes.toTypedArray())

        fun pair(pair: Pair<String, String>): Writable =
            {
                val (key, value) = pair
                rustInline("$key = $value")
            }
    }
}

/** Render all attributes in this list, one after another */
fun Collection<Attribute>.render(writer: RustWriter) {
    for (attr in this) {
        attr.render(writer)
    }
}
