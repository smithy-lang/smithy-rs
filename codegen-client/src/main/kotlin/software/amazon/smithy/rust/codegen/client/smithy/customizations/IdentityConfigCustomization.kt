/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType

class IdentityConfigCustomization(private val codegenContext: ClientCodegenContext) : ConfigCustomization() {
    override fun section(section: ServiceConfig): Writable = writable {
        if (section is ServiceConfig.ConfigImpl) {
            rustTemplate(
                """
                /// Returns the identity resolvers.
                pub fn identity_resolvers(&self) -> #{IdentityResolvers} {
                    #{ConfigBagAccessors}::identity_resolvers(&self.inner)
                }
                """,
                "IdentityResolvers" to RuntimeType.smithyRuntimeApi(codegenContext.runtimeConfig)
                    .resolve("client::identity::IdentityResolvers"),
                "ConfigBagAccessors" to RuntimeType.configBagAccessors(codegenContext.runtimeConfig),
            )
        }
    }
}
