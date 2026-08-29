/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols

import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.protocols.ProtocolCodegenModules
import software.amazon.smithy.rust.codegen.core.smithy.protocols.ProtocolGeneratorFactory
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.ServerProtocolSelectionMetadata
import software.amazon.smithy.rust.codegen.server.smithy.ServerRustModule
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerProtocol
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerProtocolGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.serverProtocolOperationsModule
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.serverProtocolRootModule

/** Modules that contain one protocol's generated serialization, deserialization, and protocol test code. */
internal data class ServerProtocolModules(
    val operations: RustModule.LeafModule,
    val protocolCodegenModules: ProtocolCodegenModules,
    val protocolTests: RustModule.LeafModule,
) {
    val serde: RustModule.LeafModule
        get() = protocolCodegenModules.serde

    val eventStreamSerde: RustModule.LeafModule
        get() = protocolCodegenModules.eventStreamSerde

    companion object {
        fun forProtocol(codegenContext: ServerCodegenContext): ServerProtocolModules =
            forProtocol(codegenContext.protocol, codegenContext.protocolSelectionMetadata)

        fun forProtocol(
            protocolId: ShapeId,
            selectionMetadata: ServerProtocolSelectionMetadata,
        ): ServerProtocolModules {
            val isMultiProtocol = selectionMetadata.isMultiProtocol
            val protocolCodegenModules =
                if (isMultiProtocol) {
                    ProtocolCodegenModules.under(serverProtocolRootModule(protocolId))
                } else {
                    ProtocolCodegenModules.Default
                }
            return ServerProtocolModules(
                operations =
                    if (isMultiProtocol) {
                        serverProtocolOperationsModule(protocolId)
                    } else {
                        ServerRustModule.Operation
                    },
                protocolCodegenModules = protocolCodegenModules,
                protocolTests =
                    if (isMultiProtocol) {
                        RustModule.private("protocol_tests_${protocolId.name.toSnakeCase()}").cfgTest()
                    } else {
                        ServerRustModule.Operation
                    },
            )
        }
    }
}

/** Information needed to generate a protocol-specific conversion from `ConstraintViolation` to `RequestRejection`. */
internal data class ConstraintViolationToRequestRejectionConversion(
    val protocol: ServerProtocol,
    val destinationModule: RustModule.LeafModule,
)

/** All code generation state for one selected server protocol. */
internal data class SelectedServerProtocol(
    val factory: ProtocolGeneratorFactory<ServerProtocolGenerator, ServerCodegenContext>,
    val context: ServerCodegenContext,
    val generator: ServerProtocolGenerator,
    val modules: ServerProtocolModules,
) {
    val protocol: ServerProtocol = generator.protocol
    val constraintViolationToRequestRejectionConversion: ConstraintViolationToRequestRejectionConversion =
        ConstraintViolationToRequestRejectionConversion(protocol, modules.operations)
}

/** Loader-supported protocols selected for a service, in canonical detection order. */
internal class ServerProtocolSelection(
    selection: List<SelectedServerProtocol>,
    val metadata: ServerProtocolSelectionMetadata,
) : List<SelectedServerProtocol> by selection {
    init {
        require(selection.isNotEmpty()) { "At least one server protocol must be selected" }
        check(selection.map { it.protocol.protocolShapeId } == metadata.protocolIds) {
            "Selected protocols must match the canonical protocol ID list"
        }
        check(selection.all { it.context.protocolSelectionMetadata === metadata }) {
            "Selected protocol contexts must share the selection metadata"
        }
    }

    /**
     * The first protocol in canonical detection order, which is checked first at runtime. Its context and factory
     * preserve legacy single-protocol extension points, and its generator emits protocol-independent operation
     * supporting types once.
     */
    val primary: SelectedServerProtocol = selection.first()
}

/** The protocol currently being rendered by [PerProtocolCodegenRenderer]. */
internal data class ProtocolRenderContext<T>(
    val protocol: T,
    val index: Int,
    val protocolCount: Int,
) {
    val isPrimary: Boolean = index == 0
}

/**
 * Renders a block once for every selected protocol.
 *
 * Each invocation writes directly to its selected destination module. Protocol serde and event-stream dependencies
 * are already configured on that protocol's codegen context.
 */
internal class PerProtocolCodegenRenderer<T>(
    private val rustCrate: RustCrate,
    private val protocols: List<T>,
) {
    init {
        require(protocols.isNotEmpty()) { "At least one protocol is required for protocol-scoped rendering" }
    }

    fun renderEach(
        destinationModule: RustModule,
        block: RustWriter.(ProtocolRenderContext<T>) -> Unit,
    ) = renderEach({ destinationModule }, block)

    fun renderEach(
        destinationModule: (T) -> RustModule,
        block: RustWriter.(ProtocolRenderContext<T>) -> Unit,
    ) {
        protocols.forEachIndexed { index, protocol ->
            val context = ProtocolRenderContext(protocol, index, protocols.size)
            val protocolDestinationModule = destinationModule(protocol)
            rustCrate.withModule(protocolDestinationModule) {
                block(context)
            }
        }
    }
}
