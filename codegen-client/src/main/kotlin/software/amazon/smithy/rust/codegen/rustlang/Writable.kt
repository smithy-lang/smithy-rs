/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.rustlang

import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.generators.GenericsGenerator

typealias Writable = RustWriter.() -> Unit

/**
 * Helper to allow coercing the Writeable signature
 * writable { rust("fn foo() { }")
 */
fun writable(w: Writable): Writable = w

fun writable(w: String): Writable = writable { rust(w) }

fun Writable.isEmpty(): Boolean {
    val writer = RustWriter.root()
    this(writer)
    return writer.toString() == RustWriter.root().toString()
}

/**
 * Helper allowing a `Iterable<Writable>` to be joined together using a separator.
 */
fun Iterable<Writable>.join(separator: String = ", ") = join(writable(separator))
fun Iterable<Writable>.join(separator: Writable = writable { }): Writable {
    val iter = this.iterator()
    return writable {
        iter.forEach { value ->
            value()
            if (iter.hasNext()) {
                separator()
            }
        }
    }
}
fun Sequence<Writable>.join(separator: String = ", ") = asIterable().join(separator)
fun Sequence<Writable>.join(separator: Writable = writable { }) = asIterable().join(separator)
fun Array<Writable>.join(separator: String = ", ") = asIterable().join(separator)
fun Array<Writable>.join(separator: Writable = writable { }) = asIterable().join(separator)

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
 *         runtimeConfig.smithyHttp().member("body::SdkBody"),
 *         GenericsGenerator(GenericTypeArg("A"), GenericTypeArg("B")),
 *     )
 * )
 * ```
 * would write out something like:
 * ```rust
 * some_fn::<crate::operation::SomeOperation, (), aws_smithy_http::body::SdkBody, A, B>();
 * ```
 */
fun rustTypeParameters(
    vararg typeParameters: Any,
): Writable = writable {
    if (typeParameters.isNotEmpty()) {
        rustInlineTemplate("<")

        val iterator: Iterator<Any> = typeParameters.iterator()
        while (iterator.hasNext()) {
            when (val typeParameter = iterator.next()) {
                is Symbol, is RuntimeType, is RustType -> rustInlineTemplate("#{it}", "it" to typeParameter)
                is String -> rustInlineTemplate(typeParameter)
                is GenericsGenerator -> rustInlineTemplate(
                    "#{gg:W}",
                    "gg" to typeParameter.declaration(withAngleBrackets = false),
                )
                else -> {
                    // Check if it's a writer. If it is, invoke it; Else, throw a codegen error.
                    val func = typeParameter as? RustWriter.() -> Unit
                    if (func != null) {
                        func.invoke(this)
                    } else {
                        throw CodegenException("Unhandled type '$typeParameter' encountered by rustTypeParameters writer")
                    }
                }
            }

            if (iterator.hasNext()) {
                rustInlineTemplate(", ")
            }
        }

        rustInlineTemplate(">")
    }
}
