/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

// TODO(AuthAlignment): Remove this file once the codegen is switched to use `AuthDecorator`

package software.amazon.smithy.rust.codegen.client.smithy.generators

import software.amazon.smithy.model.knowledge.ServiceIndex
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.traits.OptionalAuthTrait
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customizations.noAuthSchemeShapeId
import software.amazon.smithy.rust.codegen.client.smithy.customize.AuthSchemeOption
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.isEmpty
import software.amazon.smithy.rust.codegen.core.rustlang.join
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.util.PANIC
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import java.util.logging.Logger

class AuthOptionsPluginGenerator(private val codegenContext: ClientCodegenContext) {
    private val logger: Logger = Logger.getLogger(javaClass.name)

    fun authPlugin(
        pluginType: RuntimeType,
        operationShape: OperationShape,
        authSchemeOptions: List<AuthSchemeOption>,
    ) = writable {
        rustTemplate(
            """
            #{pluginType}::new(vec![#{options}])

            """,
            "pluginType" to pluginType,
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
                        is AuthSchemeOption.EndpointBasedAuthSchemeOption -> {
                            // Code generation should skip this auth scheme and not pass it to the auth scheme resolver.
                            // Since this is endpoint-based, `EndpointBasedAuthSchemeOptionResolver` will dynamically
                            // generate the auth scheme option from an endpoint at runtime.
                            false
                        }
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
        if (operationShape.hasTrait<OptionalAuthTrait>() ||
            // the file will be removed anyway but this is a workaround for CI to pass S3 tests
            codegenContext.serviceShape.hasTrait<OptionalAuthTrait>() ||
            noSupportedAuthSchemes
        ) {
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
