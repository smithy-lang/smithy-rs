/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.traits.Trait
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.auth.AuthSchemeOption
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType

/**
 * Synthetic auth trait representing `noAuth`.
 *
 * This trait should not be used generally — it was specifically introduced to revise a workaround
 * in the S3's model transformer. Previously, the transformer added `@optionalAuth` to every operation.
 * However, that would cause all operations to appear in the `operation_overrides` field of the default
 * auth resolver when querying the `AuthIndex`, since `AuthIndex` uses the presence of the
 * `optionalAuth` trait to determine whether overrides are needed.
 *
 * Ideally, we would annotate the service shape instead, but the `optionalAuth` trait is only
 * allowed on operation shapes, so we introduce this synthetic trait instead.
 */
class SyntheticNoAuthTrait : Trait {
    val ID = ShapeId.from("software.amazon.smithy.rust.codegen.client.smithy.synthetic#NoAuth")

    override fun toNode(): Node = Node.objectNode()

    override fun toShapeId(): ShapeId = ID
}

private fun noAuthModule(codegenContext: ClientCodegenContext): RuntimeType =
    CargoDependency.smithyRuntime(codegenContext.runtimeConfig)
        .toType()
        .resolve("client::auth::no_auth")

class NoAuthDecorator : ClientCodegenDecorator {
    override val name: String = "NoAuthDecorator"
    override val order: Byte = 0

    override fun authSchemeOptions(
        codegenContext: ClientCodegenContext,
        baseAuthSchemeOptions: List<AuthSchemeOption>,
    ): List<AuthSchemeOption> = baseAuthSchemeOptions + NoAuthSchemeOption()
}

class NoAuthSchemeOption : AuthSchemeOption {
    override val authSchemeId: ShapeId = ShapeId.from("smithy.api#noAuth")

    override fun render(
        codegenContext: ClientCodegenContext,
        operation: OperationShape?,
    ) = writable {
        rustTemplate(
            """
            #{AuthSchemeOption}::from(#{NO_AUTH_SCHEME_ID})
            """,
            "AuthSchemeOption" to
                RuntimeType.smithyRuntimeApiClient(codegenContext.runtimeConfig)
                    .resolve("client::auth::AuthSchemeOption"),
            "NO_AUTH_SCHEME_ID" to noAuthModule(codegenContext).resolve("NO_AUTH_SCHEME_ID"),
        )
    }
}
