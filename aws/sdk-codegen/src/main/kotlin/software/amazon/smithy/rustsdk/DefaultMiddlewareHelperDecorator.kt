/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.config.ServiceConfig

class DefaultMiddlewareHelperDecorator : RustCodegenDecorator {
    override val name: String = "DefaultMiddlewareHelper"
    override val order: Byte = 0

    override fun configCustomizations(
        codegenContext: CodegenContext,
        baseCustomizations: List<ConfigCustomization>
    ): List<ConfigCustomization> {
        return baseCustomizations + DefaultMiddlewareHelperCustomization()
    }
}

class DefaultMiddlewareHelperCustomization() : ConfigCustomization() {
    override fun section(section: ServiceConfig) = writable {
        when (section) {
            is ServiceConfig.ConfigImpl -> rustTemplate(
                """
                pub(crate) fn default_middleware(&self) -> crate::middleware::DefaultMiddleware {
                    Default::default()
                }
                """,
            )
            else -> emptySection
        }
    }
}
