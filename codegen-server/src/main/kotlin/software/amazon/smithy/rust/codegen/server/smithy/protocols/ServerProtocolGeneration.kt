/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols

import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.protocols.ProtocolGeneratorFactory
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.ServerRustModule
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerProtocol
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerProtocolGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.serverEventStreamSerdeModule
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.serverProtocolOperationsModule
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.serverProtocolSerdeModule

/** Modules that contain one protocol's generated serialization and deserialization code. */
internal data class ServerProtocolModules(
    val operations: RustModule.LeafModule,
    val serde: RustModule.LeafModule,
    val eventStreamSerde: RustModule.LeafModule,
) {
    companion object {
        fun forProtocol(
            protocolId: ShapeId,
            isMultiProtocol: Boolean,
        ): ServerProtocolModules =
            ServerProtocolModules(
                operations =
                    if (isMultiProtocol) {
                        serverProtocolOperationsModule(protocolId)
                    } else {
                        ServerRustModule.Operation
                    },
                serde = serverProtocolSerdeModule(protocolId, isMultiProtocol),
                eventStreamSerde = serverEventStreamSerdeModule(protocolId, isMultiProtocol),
            )
    }
}

/** The protocol and private modules required by generators outside the protocol visitor. */
internal data class ServerProtocolTarget(
    val protocol: ServerProtocol,
    val modules: ServerProtocolModules,
)

/** All code generation state for one selected server protocol. */
internal data class SelectedServerProtocol(
    val factory: ProtocolGeneratorFactory<ServerProtocolGenerator, ServerCodegenContext>,
    val context: ServerCodegenContext,
    val generator: ServerProtocolGenerator,
    val modules: ServerProtocolModules,
) {
    val protocol: ServerProtocol = generator.protocol
    val target: ServerProtocolTarget = ServerProtocolTarget(protocol, modules)
}

/** Loader-supported protocols selected for a service, in canonical detection order. */
internal class SelectedServerProtocols(
    protocols: List<SelectedServerProtocol>,
) : List<SelectedServerProtocol> by protocols {
    init {
        require(protocols.isNotEmpty()) { "At least one server protocol must be selected" }
    }

    val primary: SelectedServerProtocol = protocols.first()
    val isMultiProtocol: Boolean = protocols.size > 1
}

/** The protocol currently being rendered by [ProtocolScopedRenderer]. */
internal data class ProtocolRenderScope<T>(
    val protocol: T,
    val index: Int,
    val protocolCount: Int,
) {
    val isPrimary: Boolean = index == 0
}

/**
 * Renders a block once for every selected protocol.
 *
 * Single-protocol generation writes directly to the destination writer to preserve legacy output. Multi-protocol
 * generation captures each invocation and relocates fixed serde modules and their recursive inline dependencies into
 * that protocol's modules. The block must emit source and dependencies through its supplied [RustWriter].
 */
internal class ProtocolScopedRenderer<T>(
    private val rustCrate: RustCrate,
    private val protocols: List<T>,
    private val modulesFor: (T) -> ServerProtocolModules,
    debugMode: Boolean,
) {
    private val transformer = ServerProtocolCodegenTransformer(rustCrate, debugMode)

    init {
        require(protocols.isNotEmpty()) { "At least one protocol is required for protocol-scoped rendering" }
    }

    fun renderEach(
        destinationModule: RustModule,
        block: RustWriter.(ProtocolRenderScope<T>) -> Unit,
    ) = renderEach({ destinationModule }, block)

    fun renderEach(
        destinationModule: (T) -> RustModule,
        block: RustWriter.(ProtocolRenderScope<T>) -> Unit,
    ) {
        protocols.forEachIndexed { index, protocol ->
            val scope = ProtocolRenderScope(protocol, index, protocols.size)
            val protocolDestinationModule = destinationModule(protocol)
            rustCrate.withModule(protocolDestinationModule) {
                if (protocols.size == 1) {
                    block(scope)
                } else {
                    transformer.render(
                        destinationWriter = this,
                        destinationModule = protocolDestinationModule,
                        protocolModules = modulesFor(protocol),
                    ) {
                        block(scope)
                    }
                }
            }
        }
    }
}
