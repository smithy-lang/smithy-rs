/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators

import software.amazon.smithy.model.knowledge.ServiceIndex
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.traits.OptionalAuthTrait
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customizations.noAuthSchemeShapeId
import software.amazon.smithy.rust.codegen.client.smithy.customize.AuthSchemeOption
import software.amazon.smithy.rust.codegen.core.rustlang.InlineDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.isEmpty
import software.amazon.smithy.rust.codegen.core.rustlang.join
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.toType
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.util.PANIC
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import java.util.logging.Logger

private fun authPlugin(
    name: String,
    vararg additionalDependency: RustDependency,
) = InlineDependency.forRustFile(
    RustModule.pubCrate(
        name,
        parent = RustModule.private("auth_plugin"),
    ),
    "/inlineable/src/auth_plugin/$name.rs",
    *additionalDependency,
)

class AuthOptionsPluginGenerator(private val codegenContext: ClientCodegenContext) {
    private val logger: Logger = Logger.getLogger(javaClass.name)

    fun defaultAuthPlugin(
        operationShape: OperationShape,
        authSchemeOptions: List<AuthSchemeOption>,
    ) = writable {
        rustTemplate(
            """
            #{DefaultAuthOptionsPlugin}::new(vec![#{options}])

            """,
            "DefaultAuthOptionsPlugin" to authPlugin("default").toType().resolve("DefaultAuthOptionsPlugin"),
            "options" to actualAuthSchemes(operationShape, authSchemeOptions).join(", "),
        )
    }

    fun endpointBasedAuthPlugin(
        operationShape: OperationShape,
        authSchemeOptions: List<AuthSchemeOption>,
    ) = writable {
        rustTemplate(
            """
            #{EndpointBasedAuthOptionsPlugin}::new(vec![#{options}])

            """,
            "EndpointBasedAuthOptionsPlugin" to
                authPlugin("endpoint_based").toType()
                    .resolve("EndpointBasedAuthOptionsPlugin"),
            "options" to actualAuthSchemes(operationShape, authSchemeOptions).join(", "),
        )
    }

    private fun actualAuthSchemes(
        operationShape: OperationShape,
        authSchemeOptions: List<AuthSchemeOption>,
    ): List<Writable> {
        val out: MutableList<Writable> = mutableListOf()

        var noSupportedAuthSchemes = true
        val authSchemes =
            ServiceIndex.of(codegenContext.model)
                .getEffectiveAuthSchemes(codegenContext.serviceShape, operationShape)

        for (schemeShapeId in authSchemes.keys) {
            val optionsForScheme =
                authSchemeOptions.filter {
                    when (it) {
                        is AuthSchemeOption.CustomResolver -> false
                        is AuthSchemeOption.StaticAuthSchemeOption -> {
                            it.schemeShapeId == schemeShapeId
                        }
                    }
                }

            if (optionsForScheme.isNotEmpty()) {
                out.addAll(optionsForScheme.flatMap { (it as AuthSchemeOption.StaticAuthSchemeOption).constructor })
                noSupportedAuthSchemes = false
            } else {
                logger.warning(
                    "No auth scheme implementation available for $schemeShapeId. " +
                        "The generated client will not attempt to use this auth scheme.",
                )
            }
        }
        if (operationShape.hasTrait<OptionalAuthTrait>() || noSupportedAuthSchemes) {
            val authOption =
                authSchemeOptions.find {
                    it is AuthSchemeOption.StaticAuthSchemeOption && it.schemeShapeId == noAuthSchemeShapeId
                }
                    ?: throw IllegalStateException("Missing 'no auth' implementation. This is a codegen bug.")
            out += (authOption as AuthSchemeOption.StaticAuthSchemeOption).constructor
        }
        if (out.any { it.isEmpty() }) {
            PANIC("Got an empty auth scheme constructor. This is a bug. $out")
        }
        return out
    }
}
