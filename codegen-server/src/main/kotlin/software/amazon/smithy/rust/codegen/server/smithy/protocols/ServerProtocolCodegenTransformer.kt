/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols

import software.amazon.smithy.rust.codegen.core.rustlang.InlineDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.protocols.ProtocolFunctions

/**
 * Isolates one server protocol's generated code from other protocols in the same crate.
 *
 * Core protocol generators use fixed `protocol_serde` and `event_stream_serde` module names. Rendering multiple
 * protocols directly into one crate would therefore discard later inline dependencies that have the same module and
 * name. This class renders one protocol into temporary writers, recursively materializes its lazy inline dependencies,
 * and then moves the resulting source into protocol-specific modules before the real crate sees those dependencies.
 */
internal class ServerProtocolCodegenTransformer(
    private val rustCrate: RustCrate,
    debugMode: Boolean,
) {
    private val writerFactory = RustWriter.factory(debugMode)
    private val renderedDependencies = mutableSetOf<String>()

    fun renderOperation(
        operationWriter: RustWriter,
        protocolSerdeModule: RustModule.LeafModule,
        eventStreamSerdeModule: RustModule.LeafModule,
        writable: Writable,
    ) {
        val temporaryWriter =
            writerFactory.apply(
                "src/operation.rs",
                operationWriter.namespace,
            )
        writable(temporaryWriter)

        val roots =
            mapOf(
                ProtocolFunctions.serDeModule to protocolSerdeModule,
                LEGACY_EVENT_STREAM_SERDE_MODULE to eventStreamSerdeModule,
            )
        operationWriter.writeWithNoFormatting(rewrite(temporaryWriter.generatedBody(), roots))
        materializeDependencies(operationWriter, temporaryWriter.dependencies.map(RustDependency::fromSymbolDependency), roots)
    }

    private fun materializeDependencies(
        operationWriter: RustWriter,
        initialDependencies: List<RustDependency>,
        roots: Map<RustModule.LeafModule, RustModule.LeafModule>,
    ) {
        val pending = ArrayDeque(initialDependencies)

        while (pending.isNotEmpty()) {
            when (val dependency = pending.removeFirst()) {
                is InlineDependency -> {
                    val destinationModule = remap(dependency.module, roots)
                    val destinationKey = "${destinationModule.fullyQualifiedPath()}::${dependency.name}"
                    if (!renderedDependencies.add(destinationKey)) {
                        continue
                    }

                    val temporaryWriter =
                        writerFactory.apply(
                            dependency.module.definitionFile(),
                            dependency.module.fullyQualifiedPath(),
                        )
                    dependency.renderer(temporaryWriter)
                    pending.addAll(dependency.dependencies())
                    pending.addAll(temporaryWriter.dependencies.map(RustDependency::fromSymbolDependency))

                    val source = rewrite(temporaryWriter.generatedBody(), roots)
                    rustCrate.withModule(destinationModule) {
                        writeWithNoFormatting(source)
                        temporaryWriter.dependencies
                            .map(RustDependency::fromSymbolDependency)
                            .filterNot { it is InlineDependency }
                            .forEach(::addDependency)
                    }
                }
                else -> operationWriter.addDependency(dependency)
            }
        }
    }

    private fun remap(
        module: RustModule,
        roots: Map<RustModule.LeafModule, RustModule.LeafModule>,
    ): RustModule =
        roots[module]
            ?: when (module) {
                RustModule.LibRs -> RustModule.LibRs
                is RustModule.LeafModule -> module.copy(parent = remap(module.parent, roots))
            }

    private fun rewrite(
        source: String,
        roots: Map<RustModule.LeafModule, RustModule.LeafModule>,
    ): String =
        roots.entries.fold(source) { rewritten, (sourceRoot, destinationRoot) ->
            val sourcePath = Regex.escape(sourceRoot.fullyQualifiedPath())
            Regex("$sourcePath(?![A-Za-z0-9_])")
                .replace(rewritten, destinationRoot.fullyQualifiedPath())
        }

    private fun RustWriter.generatedBody(): String = toString().removePrefix("$GENERATED_WARNING\n")

    private companion object {
        const val GENERATED_WARNING = "// Code generated by software.amazon.smithy.rust.codegen.smithy-rs. DO NOT EDIT."
        val LEGACY_EVENT_STREAM_SERDE_MODULE = RustModule.private("event_stream_serde")
    }
}
