/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.customize

import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.customize.ConditionalDecorator

/** Only apply this decorator to the given service ID */
fun ClientCodegenDecorator.onlyApplyTo(serviceId: String): List<ClientCodegenDecorator> =
    listOf(
        ConditionalDecorator(this) { _, serviceShapeId ->
            serviceShapeId == ShapeId.from(serviceId)
        },
    )

/** Only apply this decorator to the given list of service IDs */
fun ClientCodegenDecorator.onlyApplyToList(serviceIds: List<String>): List<ClientCodegenDecorator> =
    serviceIds.map {
        ConditionalDecorator(this) { _, serviceShapeId ->
            serviceShapeId == ShapeId.from(it)
        }
    }

/** Apply this decorator to all services but the one identified by ID */
fun ClientCodegenDecorator.applyExceptFor(serviceId: String): List<ClientCodegenDecorator> =
    listOf(
        ConditionalDecorator(this) { _, serviceShapeId ->
            serviceShapeId != ShapeId.from(serviceId)
        },
    )

/** Apply the given decorators only to this service ID */
fun String.applyDecorators(vararg decorators: ClientCodegenDecorator): List<ClientCodegenDecorator> =
    decorators.map { it.onlyApplyTo(this) }.flatten()
