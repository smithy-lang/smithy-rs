/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators.protocol

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.server.smithy.protocols.ServerHttpBoundProtocolTraitImplGenerator

open class ServerProtocolGenerator(
    val protocol: ServerProtocol,
    private val traitGenerator: ServerHttpBoundProtocolTraitImplGenerator,
) {
    /**
     * The server implementation uses this method to generate implementations of the `from_request` and `into_response`
     * traits for operation input and output shapes, respectively.
     */
    fun renderOperation(
        operationWriter: RustWriter,
        operationShape: OperationShape,
    ) {
        traitGenerator.generateTraitImpls(operationWriter, operationShape)
    }
}
