/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.AuthSchemeOption
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.client.smithy.auth.AuthSchemeOption as AuthSchemeOptionV2

val noAuthSchemeShapeId: ShapeId = ShapeId.from("aws.smithy.rs#NoAuth")

private fun noAuthModule(codegenContext: ClientCodegenContext): RuntimeType =
    CargoDependency.smithyRuntime(codegenContext.runtimeConfig)
        .toType()
        .resolve("client::auth::no_auth")

class NoAuthDecorator : ClientCodegenDecorator {
    override val name: String = "NoAuthDecorator"
    override val order: Byte = 0

    override fun authSchemeOptions(
        codegenContext: ClientCodegenContext,
        baseAuthSchemeOptions: List<AuthSchemeOptionV2>,
    ): List<AuthSchemeOptionV2> = baseAuthSchemeOptions + NoAuthSchemeOption()

    override fun authOptions(
        codegenContext: ClientCodegenContext,
        operationShape: OperationShape,
        baseAuthSchemeOptions: List<AuthSchemeOption>,
    ): List<AuthSchemeOption> =
        baseAuthSchemeOptions +
            AuthSchemeOption.StaticAuthSchemeOption(
                noAuthSchemeShapeId,
                listOf(
                    writable {
                        rustTemplate(
                            "#{NO_AUTH_SCHEME_ID}",
                            "NO_AUTH_SCHEME_ID" to noAuthModule(codegenContext).resolve("NO_AUTH_SCHEME_ID"),
                        )
                    },
                ),
            )
}

class NoAuthSchemeOption : AuthSchemeOptionV2 {
    override val authSchemeId: ShapeId = ShapeId.from("smithy.api#noAuth")

    override fun render(
        codegenContext: ClientCodegenContext,
        operation: OperationShape?,
    ) = writable {
        rustTemplate(
            """
            #{AuthSchemeOption}::builder()
                .scheme_id(#{NO_AUTH_SCHEME_ID})
                .build()
                .expect("required fields set")
            """,
            "AuthSchemeOption" to
                RuntimeType.smithyRuntimeApiClient(codegenContext.runtimeConfig)
                    .resolve("client::auth::AuthSchemeOption"),
            "NO_AUTH_SCHEME_ID" to
                RuntimeType.smithyRuntime(codegenContext.runtimeConfig)
                    .resolve("client::auth::no_auth::NO_AUTH_SCHEME_ID"),
        )
    }
}
