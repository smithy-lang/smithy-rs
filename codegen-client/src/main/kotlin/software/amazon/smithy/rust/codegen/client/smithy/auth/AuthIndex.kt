/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.auth

import software.amazon.smithy.aws.traits.auth.UnsignedPayloadTrait
import software.amazon.smithy.model.knowledge.ServiceIndex
import software.amazon.smithy.model.knowledge.TopDownIndex
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.traits.AuthTrait
import software.amazon.smithy.model.traits.OptionalAuthTrait
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customizations.NoAuthSchemeOption
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import java.util.logging.Logger

class AuthIndex(private val ctx: ClientCodegenContext) {
    private val logger = Logger.getLogger(javaClass.name)

    /**
     * Get the Map of [AuthSchemeHandler]'s registered. The returned map is de-duplicated by
     * scheme ID with the last integration taking precedence. This map is not yet reconciled with the
     * auth schemes used by the model.
     */
    fun authOptions(): Map<ShapeId, AuthSchemeOption> =
        ctx.rootDecorator.authSchemeOptions(ctx, emptyList())
            .associateBy {
                it.shapeId
            }

    /**
     * Get the prioritized list of effective [AuthSchemeHandler] for a service (auth handlers reconciled with the
     * `auth([]` trait).
     *
     * @param ctx the generation context
     * @return the prioritized list of handlers for [ProtocolGenerator.GenerationContext.service]
     */
    fun effectiveAuthOptionsForService(): List<AuthSchemeOption> {
        val serviceIndex = ServiceIndex.of(ctx.model)
        val allAuthOptions = authOptions()

        val effectiveAuthSchemes =
            serviceIndex.getEffectiveAuthSchemes(ctx.serviceShape)
                .takeIf { it.isNotEmpty() } ?: listOf(NoAuthSchemeOption()).associateBy(NoAuthSchemeOption::shapeId)

        return effectiveAuthSchemes.mapNotNull {
            allAuthOptions[it.key]
        }
    }

    /**
     * Get the prioritized list of effective [AuthSchemeHandler] for an operation.
     *
     * @param op the operation to get auth handlers for
     * @return the prioritized list of handlers for [op]
     */
    fun effectiveAuthOptionsForOperation(op: OperationShape): List<AuthSchemeOption> {
        val serviceIndex = ServiceIndex.of(ctx.model)
        val allAuthOptions = authOptions()

        // anonymous auth (optionalAuth trait) is handled as an annotation trait...
        val opEffectiveAuthSchemes = serviceIndex.getEffectiveAuthSchemes(ctx.serviceShape, op)
        return if (op.hasTrait<OptionalAuthTrait>() || opEffectiveAuthSchemes.isEmpty()) {
            listOf(NoAuthSchemeOption())
        } else {
            // return handlers in same order as the priority list dictated by `auth([])` trait
            opEffectiveAuthSchemes.mapNotNull {
                allAuthOptions[it.key]
            }
        }
    }

    /**
     * Get the set of operations that need overridden in the generated auth scheme resolver.
     */
    fun operationsWithOverrides(): Set<OperationShape> =
        TopDownIndex.of(ctx.model)
            .getContainedOperations(ctx.serviceShape)
            .filter { op ->
                op.hasTrait<AuthTrait>() || op.hasTrait<UnsignedPayloadTrait>() || op.hasTrait<OptionalAuthTrait>()
            }.toSet()
}
