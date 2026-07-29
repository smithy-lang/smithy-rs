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

/**
 * Only apply this decorator to services whose shape ID is in the given namespace.
 *
 * This is useful for services that periodically bump their API version, which is encoded in the
 * service shape name (e.g. `com.amazonaws.route53#AWSDnsV20130401` becoming
 * `com.amazonaws.route53#AWSDnsV20130527`). Matching on the namespace keeps the decorator applied
 * across such version bumps, since exactly one service is defined per model file.
 */
fun ClientCodegenDecorator.onlyApplyToNamespace(namespace: String): List<ClientCodegenDecorator> =
    listOf(
        ConditionalDecorator(this) { _, serviceShapeId ->
            serviceShapeId?.toShapeId()?.namespace == namespace
        },
    )

/** Only apply this decorator to the given list of service IDs */
fun ClientCodegenDecorator.onlyApplyToList(serviceIds: List<String>): List<ClientCodegenDecorator> =
    serviceIds.map {
        ConditionalDecorator(this) { _, serviceShapeId ->
            serviceShapeId == ShapeId.from(it)
        }
    }

/** Apply this decorator to all services except those identified by the given IDs */
fun ClientCodegenDecorator.applyExceptFor(vararg serviceIds: String): List<ClientCodegenDecorator> {
    val excluded = serviceIds.map { ShapeId.from(it) }.toSet()
    return listOf(
        ConditionalDecorator(this) { _, serviceShapeId ->
            serviceShapeId !in excluded
        },
    )
}

/**
 * Apply this decorator to all services except those in the given namespaces.
 *
 * Like [onlyApplyToNamespace], matching on the namespace rather than the exact shape ID keeps the
 * exclusion stable across service API version bumps, which are encoded in the service shape name
 * (e.g. `AWSSecurityTokenServiceV20110615`). Exactly one service is defined per model file, so the
 * namespace uniquely identifies the excluded service.
 */
fun ClientCodegenDecorator.applyExceptForNamespaces(vararg namespaces: String): List<ClientCodegenDecorator> {
    val excluded = namespaces.toSet()
    return listOf(
        ConditionalDecorator(this) { _, serviceShapeId ->
            serviceShapeId?.toShapeId()?.namespace !in excluded
        },
    )
}

/** Apply the given decorators only to this service ID */
fun String.applyDecorators(vararg decorators: ClientCodegenDecorator): List<ClientCodegenDecorator> =
    decorators.map { it.onlyApplyTo(this) }.flatten()
