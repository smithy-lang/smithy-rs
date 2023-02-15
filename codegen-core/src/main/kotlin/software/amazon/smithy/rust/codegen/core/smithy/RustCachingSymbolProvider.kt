/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.codegen.core.SymbolProvider
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.Shape

/**
 * Adds caching to the `toSymbol` and `toMemberName` functions using Smithy's `CachingSymbolProvider`.
 */
class RustCachingSymbolProvider(base: RustSymbolProvider) : WrappingSymbolProvider(base) {
    private val cache = SymbolProvider.cache(base)

    override fun toSymbol(shape: Shape): Symbol = cache.toSymbol(shape)
    override fun toMemberName(shape: MemberShape): String = cache.toMemberName(shape)
}
