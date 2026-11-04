/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.TopDownIndex
import software.amazon.smithy.model.shapes.ServiceShape

/**
 * Fails server code generation when the service contains no operations.
 *
 * A server SDK for a service with no operations cannot serve any request, so generating one is almost certainly a
 * modeling mistake. Operations bound through resources count, so this uses the [TopDownIndex] rather than only the
 * service's direct `operations` list.
 *
 * See https://github.com/smithy-lang/smithy-rs/issues/2363.
 */
fun validateServiceHasOperations(
    model: Model,
    service: ServiceShape,
) {
    if (TopDownIndex.of(model).getContainedOperations(service).isEmpty()) {
        throw CodegenException(
            "Service ${service.id} has no operations. A server SDK must serve at least one operation; " +
                "add an operation to the service's `operations` list, or bind one through a resource.",
        )
    }
}
