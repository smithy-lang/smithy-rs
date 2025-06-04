/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.auth

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginSection
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.customize.NamedCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.Section

/**
 * An interface that provides information relevant to `AuthSchemeOption` during code generation
 *
 * * The shape ID of auth scheme ID
 * * A method that renders itself as a type `AuthSchemeOption` in Rust
 */
interface AuthSchemeOption {
    val authSchemeId: ShapeId

    fun render(
        codegenContext: ClientCodegenContext,
        operation: OperationShape? = null,
    ): Writable
}

/**
 * Auth-related code generation customization sections
 */
sealed class AuthSection(name: String) : Section(name) {
    /**
     * Hook to add additional logic within the implementation of the
     * `crate::config::auth::ResolveAuthScheme::resolve_auth_scheme` method for the default auth scheme resolver
     */
    data object DefaultResolverAdditionalImpl : AuthSection("DefaultResolverAdditionalImpl")
}

typealias AuthCustomization = NamedCustomization<AuthSection>

/**
 * Codegen decorator that:
 * * Injects service-specific default auth scheme resolver
 * * TODO(AuthAlignment): Expand this list as we add more customizations
 */
class AuthDecorator : ClientCodegenDecorator {
    override val name: String = "Auth"
    override val order: Byte = 0

    override fun serviceRuntimePluginCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ServiceRuntimePluginCustomization>,
    ): List<ServiceRuntimePluginCustomization> {
        return baseCustomizations +
            object : ServiceRuntimePluginCustomization() {
                override fun section(section: ServiceRuntimePluginSection): Writable {
                    return when (section) {
                        is ServiceRuntimePluginSection.RegisterRuntimeComponents ->
                            writable {
                                section.registerAuthSchemeOptionResolver(this, defaultAuthSchemeResolver(codegenContext))
                            }

                        else -> emptySection
                    }
                }
            }
    }
}

// Returns default auth scheme resolver for this service
private fun defaultAuthSchemeResolver(codegenContext: ClientCodegenContext): Writable {
    val generator = AuthTypesGenerator(codegenContext)
    return writable {
        rustTemplate(
            """{
            use #{ServiceSpecificResolver};
            #{DefaultResolver}::default().into_shared_resolver()
            }""",
            "DefaultResolver" to generator.defaultAuthSchemeResolver(),
            "ServiceSpecificResolver" to generator.serviceSpecificResolveAuthSchemeTrait(),
        )
    }
}
