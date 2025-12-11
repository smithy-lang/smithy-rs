/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.customizations

import software.amazon.smithy.aws.traits.auth.SigV4Trait
import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.knowledge.ServiceIndex
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.WrappingSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.traits.SyntheticInputTrait
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.isEventStream
import software.amazon.smithy.rust.codegen.core.util.isInputEventStream
import software.amazon.smithy.rust.codegen.server.smithy.customize.ServerCodegenDecorator

/**
 * Decorator that adds SigV4 event stream unsigning support to server code generation.
 */
class SigV4EventStreamDecorator : ServerCodegenDecorator {
    override val name: String = "SigV4EventStreamDecorator"
    override val order: Byte = 0

    override fun symbolProvider(base: RustSymbolProvider): RustSymbolProvider {
        if (base.usesSigAuth()) {
            return SigV4EventStreamSymbolProvider(base)
        } else {
            return base
        }
    }
}

internal fun RustSymbolProvider.usesSigAuth(): Boolean =
    ServiceIndex.of(model).getAuthSchemes(moduleProviderContext.serviceShape!!).containsKey(SigV4Trait.ID)

/**
 * Symbol provider wrapper that modifies event stream types to support SigV4 signed messages.
 */
class SigV4EventStreamSymbolProvider(
    base: RustSymbolProvider,
) : WrappingSymbolProvider(base) {
    private val runtimeConfig = base.config.runtimeConfig

    override fun toSymbol(shape: Shape): Symbol {
        val baseSymbol = super.toSymbol(shape)
        // We only want to wrap with Event Stream types when dealing with member shapes
        if (shape is MemberShape && shape.isEventStream(model)) {
            // Determine if the member has a container that is a synthetic input or output
            val operationShape =
                model.expectShape(shape.container).let { maybeInput ->
                    val operationId =
                        maybeInput.getTrait<SyntheticInputTrait>()?.operation
                    operationId?.let { model.expectShape(it, OperationShape::class.java) }
                }
            // If we find an operation shape, then we can wrap the type
            if (operationShape != null) {
                if (operationShape.isInputEventStream(model)) {
                    return SigV4EventStreamSupportStructures.wrapInEventStreamSigV4(baseSymbol, runtimeConfig)
                }
            }
        }

        return baseSymbol
    }
}
