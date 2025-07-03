/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.auth

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType

/**
 * Class responsible for generating [RuntimeType]s used to code generate auth-related types
 */
class AuthTypesGenerator(codegenContext: ClientCodegenContext) {
    private val customizations = codegenContext.rootDecorator.authCustomizations(codegenContext, emptyList())
    private val resolverGenerator = AuthSchemeResolverGenerator(codegenContext, customizations)

    /**
     * Return [RuntimeType] for the service-specific default auth scheme resolver
     */
    fun defaultAuthSchemeResolver(): RuntimeType = resolverGenerator.defaultAuthSchemeResolver()

    /**
     * Return [RuntimeType] representing the per-service trait definition for auth scheme resolution
     */
    fun serviceSpecificResolveAuthSchemeTrait(): RuntimeType = resolverGenerator.serviceSpecificResolveAuthSchemeTrait()
}
