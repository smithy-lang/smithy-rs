/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.rustlang

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.generators.GenericsGenerator
import software.amazon.smithy.rust.codegen.util.PANIC

typealias Writable = RustWriter.() -> Unit

/** Helper to allow coercing the Writeable signature
 *  writable { rust("fn foo() { }")
 */
fun writable(w: Writable): Writable = w

fun writable(w: String): Writable = writable { rust(w) }

fun Writable.isEmpty(): Boolean {
    val writer = RustWriter.root()
    this(writer)
    return writer.toString() == RustWriter.root().toString()
}

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
 *         runtimeConfig.smithyHttp().member("body::SdkBody"),
 *         GenericsGenerator(GenericTypeArg("A"), GenericTypeArg("B")),
 *     )
 * )
 * ```
 * would write out something like:
 * ```rust
 * some_fn::<crate::operation::SomeOperation, aws_smithy_http::body::SdkBody, A, B>();
 * ```
 */
fun rustTypeParameters(
    vararg typeParameters: Any,
): Writable = writable {
    if (typeParameters.isNotEmpty()) {
        rustInline("<")

        val iterator: Iterator<Any> = typeParameters.iterator()
        while (iterator.hasNext()) {
            when (val typeParameter = iterator.next()) {
                is Symbol, RustType.Unit -> rustTemplate("#{it}", "it" to typeParameter)
                is RuntimeType -> rustTemplate("#{it:T}", "it" to typeParameter)
                is String -> rust(typeParameter)
                is GenericsGenerator -> rustTemplate(
                    "#{gg:W}",
                    "gg" to typeParameter.declaration(withAngleBrackets = false),
                )
                else -> PANIC("Unhandled type '$typeParameter' encountered by rustTypeParameters writer")
            }

            if (iterator.hasNext()) {
                rustInline(", ")
            }
        }

        rustInline(">")
    }
}
