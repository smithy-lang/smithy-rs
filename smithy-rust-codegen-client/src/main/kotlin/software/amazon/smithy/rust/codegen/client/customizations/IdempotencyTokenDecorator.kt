/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.customizations

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.client.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.client.generators.ServiceRuntimePluginCustomization
import software.amazon.smithy.rust.codegen.client.generators.ServiceRuntimePluginSection
import software.amazon.smithy.rust.codegen.client.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.generators.config.IdempotencyTokenProviderCustomization
import software.amazon.smithy.rust.codegen.client.generators.config.needsIdempotencyToken
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.RuntimeType
import software.amazon.smithy.rust.codegen.core.util.extendIf

class IdempotencyTokenDecorator : ClientCodegenDecorator {
    override val name: String = "IdempotencyToken"
    override val order: Byte = 0

    private fun enabled(ctx: ClientCodegenContext) = ctx.serviceShape.needsIdempotencyToken(ctx.model)

    override fun configCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ConfigCustomization>,
    ): List<ConfigCustomization> =
        baseCustomizations.extendIf(enabled(codegenContext)) {
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
                override fun section(section: ServiceRuntimePluginSection) =
                    writable {
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
