/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators.protocol

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.ProtocolGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.ProtocolTraitImplGenerator

open class ServerProtocolGenerator(
    codegenContext: CodegenContext,
    val protocol: ServerProtocol,
    private val traitGenerator: ProtocolTraitImplGenerator,
) : ProtocolGenerator(codegenContext, protocol, traitGenerator) {
    /**
     * The server implementation uses this method to generate implementations of the `from_request` and `into_response`
     * traits for operation input and output shapes, respectively.
     */
    fun renderOperation(
        operationWriter: RustWriter,
        operationShape: OperationShape,
    ) {
        traitGenerator.generateTraitImpls(operationWriter, operationShape, emptyList())
    }
}
