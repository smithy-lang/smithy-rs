/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginSection
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.IdempotencyTokenProviderCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.needsIdempotencyToken
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.util.extendIf

class IdempotencyTokenDecorator : ClientCodegenDecorator {
    override val name: String = "IdempotencyToken"
    override val order: Byte = 0

    private fun enabled(ctx: ClientCodegenContext) = ctx.serviceShape.needsIdempotencyToken(ctx.model)
    override fun configCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ConfigCustomization>,
    ): List<ConfigCustomization> = baseCustomizations.extendIf(enabled(codegenContext)) {
        IdempotencyTokenProviderCustomization(codegenContext)
    }

    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> {
        return baseCustomizations + IdempotencyTokenGenerator(codegenContext, operation)
    }

    override fun serviceRuntimePluginCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ServiceRuntimePluginCustomization>,
    ): List<ServiceRuntimePluginCustomization> {
        return baseCustomizations.extendIf(enabled(codegenContext)) {
            object : ServiceRuntimePluginCustomization() {
                override fun section(section: ServiceRuntimePluginSection) = writable {
                    if (section is ServiceRuntimePluginSection.AdditionalConfig) {
                        section.putConfigValue(this, defaultTokenProvider((codegenContext.runtimeConfig)))
                    }
                }
            }
        }
    }
}

private fun defaultTokenProvider(runtimeConfig: RuntimeConfig) =
    writable { rust("#T()", RuntimeType.idempotencyToken(runtimeConfig).resolve("default_provider")) }
