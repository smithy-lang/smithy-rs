/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.python.smithy

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.stripOuter
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.WrappingSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.rustType
import software.amazon.smithy.rust.codegen.core.smithy.traits.SyntheticInputTrait
import software.amazon.smithy.rust.codegen.core.smithy.traits.SyntheticOutputTrait
import software.amazon.smithy.rust.codegen.core.smithy.transformers.eventStreamErrors
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.isEventStream
import software.amazon.smithy.rust.codegen.core.util.isOutputEventStream
import software.amazon.smithy.rust.codegen.core.util.toPascalCase

/**
 * Symbol provider for Python that maps event streaming member shapes to their respective Python wrapper types.
 *
 * For example given a model:
 * ```smithy
 * @input
 * structure CapturePokemonInput {
 *     @httpPayload
 *     events: AttemptCapturingPokemonEvent,
 * }
 *
 * @streaming
 * union AttemptCapturingPokemonEvent {
 *     ...
 * }
 * ```
 * for the member shape `CapturePokemonInput$events` it will return a symbol that points to
 * `crate::python_event_stream::CapturePokemonInputEventsReceiver`.
 */
class PythonEventStreamSymbolProvider(
    private val runtimeConfig: RuntimeConfig,
    base: RustSymbolProvider,
) : WrappingSymbolProvider(base) {
    override fun toSymbol(shape: Shape): Symbol {
        val initial = super.toSymbol(shape)

        // We only want to wrap with Event Stream types when dealing with member shapes
        if (shape !is MemberShape || !shape.isEventStream(model)) {
            return initial
        }

        // We can only wrap the type if it's either an input or an output that used in an operation
        model.expectShape(shape.container).let { maybeInputOutput ->
            val operationId = maybeInputOutput.getTrait<SyntheticInputTrait>()?.operation
                ?: maybeInputOutput.getTrait<SyntheticOutputTrait>()?.operation
            operationId?.let { model.expectShape(it, OperationShape::class.java) }
        } ?: return initial

        val unionShape = model.expectShape(shape.target).asUnionShape().get()
        val error = if (unionShape.eventStreamErrors().isEmpty()) {
            RuntimeType.smithyHttp(runtimeConfig).resolve("event_stream::MessageStreamError").toSymbol()
        } else {
            symbolForEventStreamError(unionShape)
        }
        val inner = initial.rustType().stripOuter<RustType.Option>()
        val innerSymbol = Symbol.builder().name(inner.name).rustType(inner).build()
        val containerName = shape.container.name
        val memberName = shape.memberName.toPascalCase()
        val outer = when (shape.isOutputEventStream(model)) {
            true -> "${containerName}${memberName}EventStreamSender"
            else -> "${containerName}${memberName}Receiver"
        }
        val rustType = RustType.Opaque(outer, PythonServerRustModule.PythonEventStream.fullyQualifiedPath())
        return Symbol.builder()
            .name(rustType.name)
            .rustType(rustType)
            .addReference(innerSymbol)
            .addReference(error)
            .addDependency(CargoDependency.smithyHttp(runtimeConfig).withFeature("event-stream"))
            .addDependency(PythonServerCargoDependency.Futures)
            .addDependency(PythonServerCargoDependency.PyO3Asyncio.withFeature("unstable-streams"))
            .build()
    }

    companion object {
        data class EventStreamSymbol(val innerT: RustType, val errorT: RustType)

        fun parseSymbol(symbol: Symbol): EventStreamSymbol {
            check(symbol.references.size >= 2) {
                "`PythonEventStreamSymbolProvider` adds inner type and error type as references to resulting symbol"
            }
            val innerT = symbol.references[0].symbol.rustType()
            val errorT = symbol.references[1].symbol.rustType()
            return EventStreamSymbol(innerT, errorT)
        }
    }
}
