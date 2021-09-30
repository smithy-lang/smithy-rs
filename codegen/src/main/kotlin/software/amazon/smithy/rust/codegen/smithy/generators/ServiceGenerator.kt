/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.generators

import software.amazon.smithy.model.knowledge.TopDownIndex
import software.amazon.smithy.rust.codegen.rustlang.RustModule
import software.amazon.smithy.rust.codegen.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.smithy.RustCrate
import software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.smithy.generators.config.ServiceConfigGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.error.CombinedErrorGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.error.TopLevelErrorGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.protocol.ProtocolGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.protocol.ProtocolSupport
import software.amazon.smithy.rust.codegen.smithy.generators.protocol.ProtocolTestGenerator
import software.amazon.smithy.rust.codegen.util.inputShape

class ServiceGenerator(
    private val rustCrate: RustCrate,
    private val protocolGenerator: ProtocolGenerator,
    private val protocolSupport: ProtocolSupport,
    private val config: CodegenContext,
    private val decorator: RustCodegenDecorator,
) {
    private val index = TopDownIndex.of(config.model)

    fun render() {
        val operations = index.getContainedOperations(config.serviceShape).sortedBy { it.id }
        operations.map { operation ->
            rustCrate.useShapeWriter(operation) { operationWriter ->
                rustCrate.useShapeWriter(operation.inputShape(config.model)) { inputWriter ->
                    protocolGenerator.renderOperation(
                        operationWriter,
                        inputWriter,
                        operation,
                        decorator.operationCustomizations(config, operation, listOf())
                    )
                    ProtocolTestGenerator(config, protocolSupport, operation, operationWriter).render()
                }
            }
            rustCrate.withModule(RustModule.Error) { writer ->
                CombinedErrorGenerator(config.model, config.symbolProvider, operation).render(writer)
            }
        }

        TopLevelErrorGenerator(config, operations).render(rustCrate)

        rustCrate.withModule(RustModule.Config) { writer ->
            ServiceConfigGenerator.withBaseBehavior(
                config,
                extraCustomizations = decorator.configCustomizations(config, listOf())
            ).render(writer)
        }

        rustCrate.lib {
            it.write("pub use config::Config;")
        }
    }
}
