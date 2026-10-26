/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.fuzz

import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.WrappingSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.rustType

/** Fuzz record signatures can contain SDK shapes nested in Vec, Option, Box, or maps. */
internal class FuzzPublicSymbols(private val base: RustSymbolProvider, private val crate: String) :
    WrappingSymbolProvider(base) {
    override fun toSymbol(shape: Shape) =
        base.toSymbol(shape).let { it.toBuilder().rustType(publicType(it.rustType())).build() }

    private fun publicType(type: RustType): RustType =
        when (type) {
            is RustType.Opaque ->
                if (type.namespace?.startsWith("crate::") == true) {
                    type.copy(namespace = type.namespace!!.replaceFirst("crate::", "$crate::"))
                } else {
                    type
                }
            is RustType.Vec -> type.copy(member = publicType(type.member))
            is RustType.HashMap -> type.copy(key = publicType(type.key), member = publicType(type.member))
            is RustType.HashSet -> type.copy(member = publicType(type.member))
            is RustType.Slice -> type.copy(member = publicType(type.member))
            is RustType.Option -> type.copy(member = publicType(type.member))
            is RustType.Box -> type.copy(member = publicType(type.member))
            is RustType.Reference -> type.copy(member = publicType(type.member))
            is RustType.Dyn -> type.copy(member = publicType(type.member))
            is RustType.MaybeConstrained -> type.copy(member = publicType(type.member))
            is RustType.Application -> type.copy(type = publicType(type.type), args = type.args.map(::publicType))
            else -> type
        }
}
