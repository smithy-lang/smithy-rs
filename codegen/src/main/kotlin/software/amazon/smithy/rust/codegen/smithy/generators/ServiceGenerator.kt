/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.generators

import software.amazon.smithy.codegen.core.writer.CodegenWriterDelegator
import software.amazon.smithy.model.knowledge.TopDownIndex
import software.amazon.smithy.rust.codegen.lang.RustWriter

class ServiceGenerator(
    private val writers: CodegenWriterDelegator<RustWriter>,
    private val protocolGenerator: HttpProtocolGenerator,
    private val config: ProtocolConfig
) {
    private val index = TopDownIndex(config.model)

    fun render() {
        val operations = index.getContainedOperations(config.serviceShape)
        // TODO: refactor so that we don't need to re-instantiate the protocol for every operation
        operations.map { operation ->
            writers.useShapeWriter(operation) { writer ->
                protocolGenerator.renderOperation(writer, operation)
                HttpProtocolTestGenerator(config, operation, writer).render()
            }
        }
    }
}
