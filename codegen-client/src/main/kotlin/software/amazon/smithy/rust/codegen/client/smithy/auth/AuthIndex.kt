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

/**
 * Knowledge index for dealing with authentication traits and AuthSchemeOptions
 *
 * This class is a port of the Kotlin SDK counterpart:
 * https://github.com/smithy-lang/smithy-kotlin/blob/main/codegen/smithy-kotlin-codegen/src/main/kotlin/software/amazon/smithy/kotlin/codegen/model/knowledge/AuthIndex.kt
 */
class AuthIndex(private val ctx: ClientCodegenContext) {
    /**
     * Get the Map of [AuthSchemeOption]'s registered. The returned map is de-duplicated by
     * scheme ID with the last integration taking precedence. This map is not yet reconciled with the
     * auth schemes used by the model.
     */
    fun authOptions(): Map<ShapeId, AuthSchemeOption> =
        ctx.rootDecorator.authSchemeOptions(ctx, emptyList())
            .associateBy {
                it.authSchemeId
            }

    /**
     * Get the prioritized list of effective [AuthSchemeOption] for a service (auth options reconciled with the
     * `auth([]` trait).
     */
    fun effectiveAuthOptionsForService(): List<AuthSchemeOption> {
        val serviceIndex = ServiceIndex.of(ctx.model)
        val allAuthOptions = authOptions()

        val effectiveAuthSchemes =
            serviceIndex.getEffectiveAuthSchemes(ctx.serviceShape)
                .takeIf { it.isNotEmpty() } ?: listOf(NoAuthSchemeOption()).associateBy(NoAuthSchemeOption::authSchemeId)

        return effectiveAuthSchemes.mapNotNull {
            allAuthOptions[it.key]
        }
    }

    /**
     * Get the prioritized list of effective [AuthSchemeOption]s for an operation.
     */
    fun effectiveAuthOptionsForOperation(op: OperationShape): List<AuthSchemeOption> {
        val serviceIndex = ServiceIndex.of(ctx.model)
        val allAuthOptions = authOptions()

        // anonymous auth (optionalAuth trait) is handled as an annotation trait...
        val opEffectiveAuthSchemes = serviceIndex.getEffectiveAuthSchemes(ctx.serviceShape, op)
        return if (op.hasTrait<OptionalAuthTrait>() || opEffectiveAuthSchemes.isEmpty()) {
            listOf(NoAuthSchemeOption())
        } else {
            // return auth schemes in same order as the priority list dictated by `auth([])` trait
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
