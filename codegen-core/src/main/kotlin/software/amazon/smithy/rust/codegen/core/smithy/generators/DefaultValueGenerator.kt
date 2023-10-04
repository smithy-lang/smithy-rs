/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.generators

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.SimpleShape
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.Default
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.defaultValue

class DefaultValueGenerator(
    runtimeConfig: RuntimeConfig,
    private val symbolProvider: RustSymbolProvider,
    private val model: Model,
) {
    private val instantiator = PrimitiveInstantiator(runtimeConfig, symbolProvider)

    data class DefaultValue(val isRustDefault: Boolean, val expr: Writable)

    /** Returns the default value as set by the defaultValue trait */
    fun defaultValue(member: MemberShape): DefaultValue? {
        val target = model.expectShape(member.target)
        return when (val default = symbolProvider.toSymbol(member).defaultValue()) {
            is Default.NoDefault -> null
            is Default.RustDefault -> DefaultValue(isRustDefault = true, writable("Default::default"))
            is Default.NonZeroDefault -> {
                val instantiation = instantiator.instantiate(target as SimpleShape, default.value)
                DefaultValue(isRustDefault = false, writable { rust("||#T", instantiation) })
            }
        }
    }
}
