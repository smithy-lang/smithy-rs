/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginSection
import software.amazon.smithy.rust.codegen.core.rustlang.InlineDependency
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType

/**
 * A decorator for tracking Smithy SDK features that are enabled according to the model at code generation time.
 *
 * Other Smithy SDK features are typically tracked at runtime by their respective interceptors, because whether
 * they are enabled is not determined until later during the execution.
 */
class StaticSdkFeatureTrackerDecorator : ClientCodegenDecorator {
    override val name: String = "StaticSdkFeatureTrackerDecorator"
    override val order: Byte = 0

    override fun serviceRuntimePluginCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ServiceRuntimePluginCustomization>,
    ): List<ServiceRuntimePluginCustomization> =
        baseCustomizations + RpcV2CborFeatureTrackerRuntimePluginCustomization(codegenContext)
}

private class RpcV2CborFeatureTrackerRuntimePluginCustomization(private val codegenContext: ClientCodegenContext) :
    ServiceRuntimePluginCustomization() {
    private val rpcV2CborProtocolShapeId = ShapeId.from("smithy.protocols#rpcv2Cbor")

    override fun section(section: ServiceRuntimePluginSection): Writable =
        writable {
            when (section) {
                is ServiceRuntimePluginSection.RegisterRuntimeComponents -> {
                    if (codegenContext.protocol == rpcV2CborProtocolShapeId) {
                        section.registerInterceptor(this) {
                            rustTemplate(
                                "#{RpcV2CborFeatureTrackerInterceptor}::new()",
                                "RpcV2CborFeatureTrackerInterceptor" to
                                    RuntimeType.forInlineDependency(
                                        InlineDependency.sdkFeatureTracker(codegenContext.runtimeConfig),
                                    ).resolve("rpc_v2_cbor::RpcV2CborFeatureTrackerInterceptor"),
                            )
                        }
                    }
                }

                else -> emptySection
            }
        }
}
