/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.generators

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.shapes.BooleanShape
import software.amazon.smithy.model.shapes.DocumentShape
import software.amazon.smithy.model.shapes.EnumShape
import software.amazon.smithy.model.shapes.ListShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.NumberShape
import software.amazon.smithy.model.shapes.SimpleShape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.TimestampShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.some
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.Default
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.defaultValue
import software.amazon.smithy.rust.codegen.core.util.isEventStream
import software.amazon.smithy.rust.codegen.core.util.isStreaming

class DefaultValueGenerator(
    runtimeConfig: RuntimeConfig,
    private val symbolProvider: RustSymbolProvider,
    private val model: Model,
) {
    private val instantiator = PrimitiveInstantiator(runtimeConfig, symbolProvider)

    /** Returns the default value as set by the defaultValue trait */
    fun defaultValue(member: MemberShape): Writable? {
        val target = model.expectShape(member.target)
        return when (val default = symbolProvider.toSymbol(member).defaultValue()) {
            is Default.NoDefault -> null
            is Default.RustDefault -> writable("Default::default")
            is Default.NonZeroDefault -> {
                val instantiation = instantiator.instantiate(target as SimpleShape, default.value)
                writable { rust("||#T", instantiation) }
            }
        }
    }

    fun errorCorrection(member: MemberShape): Writable? {
        val symbol = symbolProvider.toSymbol(member)
        val target = model.expectShape(member.target)
        if (member.isEventStream(model) || member.isStreaming(model)) {
            return null
        }
        return writable {
            when (target) {
                is EnumShape -> rustTemplate(""""no value was set".parse::<#{Shape}>().ok()""", "Shape" to symbol)
                is BooleanShape, is NumberShape, is StringShape, is DocumentShape, is ListShape, is MapShape -> rust("Some(Default::default())")
                is StructureShape -> rust(
                    "#T::default().build_with_error_correction().ok()",
                    symbolProvider.symbolForBuilder(target),
                )
                is TimestampShape -> instantiator.instantiate(target, Node.from(0)).some()(this)
                is BlobShape -> instantiator.instantiate(target, Node.from("")).some()(this)

                is UnionShape -> rust("Some(#T::Unknown)", symbol)
            }
        }
    }
}
