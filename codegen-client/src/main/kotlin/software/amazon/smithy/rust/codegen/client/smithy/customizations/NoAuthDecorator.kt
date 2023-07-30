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
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType

val noAuthSchemeShapeId: ShapeId = ShapeId.from("aws.smithy.rs#NoAuth")

private fun noAuthModule(codegenContext: ClientCodegenContext): RuntimeType =
    CargoDependency.smithyRuntime(codegenContext.runtimeConfig)
        .toType()
        .resolve("client::auth::no_auth")

class NoAuthDecorator : ClientCodegenDecorator {
    override val name: String = "NoAuthDecorator"
    override val order: Byte = 0

    override fun authOptions(
        codegenContext: ClientCodegenContext,
        operationShape: OperationShape,
        baseAuthSchemeOptions: List<AuthSchemeOption>,
    ): List<AuthSchemeOption> = baseAuthSchemeOptions +
        AuthSchemeOption.StaticAuthSchemeOption(noAuthSchemeShapeId) {
            rustTemplate(
                "#{NO_AUTH_SCHEME_ID},",
                "NO_AUTH_SCHEME_ID" to noAuthModule(codegenContext).resolve("NO_AUTH_SCHEME_ID"),
            )
        }
}
