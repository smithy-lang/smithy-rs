/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.traits.OptionalAuthTrait
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.AuthOption
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationSection
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.letIf

val noAuthSchemeShapeId: ShapeId = ShapeId.from("aws.smithy.rs#NoAuth")

private fun noAuthModule(codegenContext: ClientCodegenContext): RuntimeType =
    CargoDependency.smithyRuntime(codegenContext.runtimeConfig)
        .withFeature("no-auth")
        .toType()
        .resolve("client::auth::no_auth")

class NoAuthDecorator : ClientCodegenDecorator {
    override val name: String = "NoAuthDecorator"
    override val order: Byte = 0

    override fun authOptions(
        codegenContext: ClientCodegenContext,
        operationShape: OperationShape,
        baseAuthOptions: List<AuthOption>,
    ): List<AuthOption> = baseAuthOptions.letIf(operationShape.hasTrait<OptionalAuthTrait>()) {
        it + AuthOption.StaticAuthOption(noAuthSchemeShapeId) {
            rustTemplate(
                "#{NO_AUTH_SCHEME_ID},",
                "NO_AUTH_SCHEME_ID" to noAuthModule(codegenContext).resolve("NO_AUTH_SCHEME_ID"),
            )
        }
    }

    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> = baseCustomizations + AnonymousAuthCustomization(codegenContext, operation)
}

class AnonymousAuthCustomization(
    private val codegenContext: ClientCodegenContext,
    private val operationShape: OperationShape,
) : OperationCustomization() {
    override fun section(section: OperationSection): Writable = writable {
        if (
            codegenContext.smithyRuntimeMode.generateOrchestrator &&
            section is OperationSection.AdditionalRuntimePlugins &&
            operationShape.hasTrait<OptionalAuthTrait>()
        ) {
            section.addOperationRuntimePlugin(this) {
                rustTemplate(
                    "#{NoAuthRuntimePlugin}::new()",
                    "NoAuthRuntimePlugin" to noAuthModule(codegenContext).resolve("NoAuthRuntimePlugin"),
                )
            }
        }
    }
}
