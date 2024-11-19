/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.client.CustomizableOperationSection
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.customize.AdHocCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.adhocCustomization

class DisablePayloadSigningDecorator : ClientCodegenDecorator {
    override val name: String = "DisablePayloadSigning"
    override val order: Byte = 0

    override fun extraSections(codegenContext: ClientCodegenContext): List<AdHocCustomization> =
        listOf(
            adhocCustomization<CustomizableOperationSection.CustomizableOperationImpl> {
                rustTemplate(
                    """
                    /// Disable payload signing for this request.
                    ///
                    /// **WARNING:** This is an advanced feature that removes
                    /// the cost of signing a request payload by removing a data
                    /// integrity check. Not all services/operations support
                    /// this feature.
                    pub fn disable_payload_signing(self) -> Self {
                        self.runtime_plugin(#{PayloadSigningOverrideRuntimePlugin}::unsigned())
                    }
                    """,
                    *preludeScope,
                    "PayloadSigningOverrideRuntimePlugin" to
                        AwsRuntimeType.awsRuntime(codegenContext.runtimeConfig)
                            .resolve("auth::PayloadSigningOverrideRuntimePlugin"),
                )
            },
        )
}
