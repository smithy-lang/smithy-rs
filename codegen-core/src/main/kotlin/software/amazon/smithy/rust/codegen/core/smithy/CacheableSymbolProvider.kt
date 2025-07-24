/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.shapes.ListShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.traits.CacheableTrait

/**
 * Wrapping symbol provider support adding Cacheable to members
 */
class CacheableSymbolProvider(private val base: RustSymbolProvider) : WrappingSymbolProvider(base) {
    private val runtimeConfig = base.config.runtimeConfig

    override fun toSymbol(shape: Shape): Symbol {
        return when (shape) {
            is ListShape ->
                base.toSymbol(shape)
                    .mapRustType { ty -> (ty as RustType.Vec).copy(member = toSymbol(shape.member).rustType()) }

            is MemberShape -> {
                val initial = base.toSymbol(shape)
                val targetShape = model.expectShape(shape.target)
                val cacheableType = RuntimeType.Cacheable.toSymbol().rustType()

                return if (shape.hasTrait(CacheableTrait::class.java)) {
                    val cacheable = RuntimeType.Cacheable
                    initial.mapRustType { initial ->
                        when (initial) {
                            is RustType.Option -> initial.map { RustType.Application(cacheableType, listOf(it)) }

                            else ->
                                RustType.Application(
                                    type = cacheableType,
                                    args = listOf(initial),
                                )
                        }
                    }.toBuilder().addReference(cacheable.toSymbol()).build()
                } else {
                    val targetSymbol = toSymbol(targetShape)
                    initial.mapRustType { memberRustType ->
                        when (memberRustType) {
                            is RustType.Container -> memberRustType.map { targetSymbol.rustType() }
                            else -> memberRustType
                        }
                    }
                }
            }

            else -> base.toSymbol(shape)
        }
    }
}
