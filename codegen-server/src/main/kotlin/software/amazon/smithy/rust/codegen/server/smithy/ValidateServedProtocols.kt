/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId

/**
 * Fails code generation when the service declares a protocol code generation does not support.
 *
 * A server serves every protocol its service declares, so an unsupported one fails the build rather than
 * being silently dropped. (Smithy's own `HttpBindingsMissing` validation already requires `@http` on every
 * operation of a service declaring a REST protocol.)
 */
fun validateServedProtocols(
    service: ServiceShape,
    servedProtocols: List<ShapeId>,
    supportedProtocols: Set<ShapeId>,
) {
    val unsupported = servedProtocols.filterNot { it in supportedProtocols }
    if (unsupported.isNotEmpty()) {
        throw CodegenException(
            "Service ${service.id} declares protocols that are not supported: $unsupported. Supported protocols: $supportedProtocols",
        )
    }
}
