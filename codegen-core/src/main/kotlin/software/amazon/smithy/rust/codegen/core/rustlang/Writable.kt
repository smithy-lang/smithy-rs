/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.rustlang

import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType

typealias Writable = RustWriter.() -> Unit

/**
 * Helper to allow coercing the Writeable signature
 * writable { rust("fn foo() { }")
 */
fun writable(w: Writable): Writable = w

fun writable(w: String): Writable = writable { writeInline(w) }

fun Writable.isEmpty(): Boolean {
    val writer = RustWriter.root()
    this(writer)
    return writer.toString() == RustWriter.root().toString()
}

fun Writable.map(f: RustWriter.(Writable) -> Unit): Writable {
    val self = this
    return writable { f(self) }
}

/** Returns Some(..arg) */
fun Writable.some(): Writable {
    return this.map { rust("Some(#T)", it) }
}

fun Writable.isNotEmpty(): Boolean = !this.isEmpty()

operator fun Writable.plus(other: Writable): Writable =
    writable {
        this@plus(this)
        other(this)
    }

/**
 * Helper allowing a `Iterable<Writable>` to be joined together using a `String` separator.
 * @param separator The string to use as a separator between elements
 * @param prefix An optional string to prepend to the entire joined sequence (defaults to null)
 * @return A Writable containing the optionally prefixed, joined elements
 */
fun Iterable<Writable>.join(
    separator: String,
    prefix: String? = null,
) = join(writable(separator), prefix?.let { writable(it) })

/**
 * Helper allowing a `Iterable<Writable>` to be joined together using a `Writable` separator.
 * @param separator The Writable to use as a separator between elements
 * @param prefix An optional Writable to prepend to the entire joined sequence (defaults to null)
 * @return A Writable containing the optionally prefixed, joined elements
 */
fun Iterable<Writable>.join(
    separator: Writable,
    prefix: Writable? = null,
): Writable {
    val iter = this.iterator()
    return writable {
        if (iter.hasNext() && prefix != null) {
            prefix()
        }
        iter.forEach { value ->
            value()
            if (iter.hasNext()) {
                separator()
            }
        }
    }
}

/**
 * Helper allowing a `Sequence<Writable>` to be joined together using a `String` separator.
 */
fun Sequence<Writable>.join(separator: String) = asIterable().join(separator)

/**
 * Helper allowing a `Sequence<Writable>` to be joined together using a `Writable` separator.
 */
fun Sequence<Writable>.join(separator: Writable) = asIterable().join(separator)

/**
 * Helper allowing a `Array<Writable>` to be joined together using a `String` separator.
 */
fun Array<Writable>.join(separator: String) = asIterable().join(separator)

/**
 * Helper allowing a `Array<Writable>` to be joined together using a `Writable` separator.
 */
fun Array<Writable>.join(separator: Writable) = asIterable().join(separator)

/**
 * Combine multiple writable types into a Rust generic type parameter list
 *
 * e.g.
 *
 * ```kotlin
 * rustTemplate(
 *     "some_fn::<#{type_params:W}>();",
 *     "type_params" to rustTypeParameters(
 *         symbolProvider.toSymbol(operation),
 *         RustType.Unit,
 *         runtimeConfig.smithyTypes().resolve("body::SdkBody"),
 *         GenericsGenerator(GenericTypeArg("A"), GenericTypeArg("B")),
 *     )
 * )
 * ```
 * would write out something like:
 * ```rust
 * some_fn::<crate::operation::SomeOperation, (), aws_smithy_types::body::SdkBody, A, B>();
 * ```
 */
fun rustTypeParameters(vararg typeParameters: Any): Writable =
    writable {
        if (typeParameters.isNotEmpty()) {
            val items =
                typeParameters.map { typeParameter ->
                    writable {
                        when (typeParameter) {
                            is Symbol, is RuntimeType, is RustType -> rustInlineTemplate("#{it}", "it" to typeParameter)
                            is String -> rustInlineTemplate(typeParameter)
                            is RustGenerics ->
                                rustInlineTemplate(
                                    "#{gg:W}",
                                    "gg" to typeParameter.declaration(withAngleBrackets = false),
                                )

                            else -> {
                                // Check if it's a writer. If it is, invoke it; Else, throw a codegen error.
                                @Suppress("UNCHECKED_CAST")
                                val func =
                                    typeParameter as? Writable
                                        ?: throw CodegenException("Unhandled type '$typeParameter' encountered by rustTypeParameters writer")
                                func.invoke(this)
                            }
                        }
                    }
                }

            rustInlineTemplate("<#{Items:W}>", "Items" to items.join(", "))
        }
    }
