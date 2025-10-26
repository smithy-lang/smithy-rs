/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.customizations

import software.amazon.smithy.rust.codegen.core.smithy.generators.ManifestCustomizations
import software.amazon.smithy.rust.codegen.core.util.hasEventStreamOperations
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.customize.ServerCodegenDecorator

/**
 * Applies HTTP 0.x specific dependency customizations.
 *
 * When generating code for HTTP 0.x (hyper 0.14), we need to:
 * 1. Pin aws-smithy-json to version 0.61 (last version without http-body-1-x requirement)
 * 2. Add http-body-0-4-x feature to aws-smithy-types
 * 3. Add event-stream feature to aws-smithy-http when event streams are used
 *
 * Version 0.62 and later of aws-smithy-json have a hard dependency on aws-smithy-types
 * with the http-body-1-x feature, which is incompatible with HTTP 0.x.
 *
 * For event streams: The event-stream module in aws-smithy-http 0.62 is behind a feature
 * flag, so we need to explicitly enable it when the service uses event stream operations.
 */
class Http0xDependencyPinningDecorator : ServerCodegenDecorator {
    override val name: String = "Http0xDependencyPinning"
    override val order: Byte = 0

    override fun crateManifestCustomizations(codegenContext: ServerCodegenContext): ManifestCustomizations {
        // Debug: Print HTTP version configuration
        println("[Http0xDependencyPinning] http1x setting: ${codegenContext.isHttp1()}")

        // Only apply customizations for HTTP 0.x
        if (codegenContext.isHttp1()) {
            println("[Http0xDependencyPinning] HTTP 1.x detected, skipping customizations")
            return emptyMap()
        }

        println("[Http0xDependencyPinning] HTTP 0.x detected, applying customizations")

        // Get dependencies to pin from HttpDependencies
        val dependenciesToPin = codegenContext.httpDependencies().dependenciesToPin().toMutableMap()

        // Explicitly add aws-smithy-types with only the correct http-body feature
        // This ensures that even if core codegen adds it without features or with http-body-1-x,
        // our explicit version with http-body-0-4-x will take precedence
        val smithyTypes = codegenContext.httpDependencies().smithyTypes
        dependenciesToPin["aws-smithy-types"] = smithyTypes

        // Check if the service has event stream operations to enable `event-stream` feature.
        val serviceShape = codegenContext.serviceShape
        val hasEventStreams = serviceShape.hasEventStreamOperations(codegenContext.model)

        // If event streams are used, ensure aws-smithy-http has the event-stream feature.
        // Note: EventStreamSymbolProvider (in codegen-core) adds aws-smithy-http dependency
        // to symbols, but it uses CargoDependency.smithyHttp(runtimeConfig) which gives the
        // default version, not our HTTP 0.x pinned version. By adding it here with the
        // correct pinned version and event-stream feature, our manifest customization will
        // override the default version while preserving the feature requirement.
        if (hasEventStreams) {
            val smithyHttp = codegenContext.httpDependencies().smithyHttp.withFeature("event-stream")
            dependenciesToPin["aws-smithy-http"] = smithyHttp
        }

        // Build dependency customizations
        val dependencyCustomizations =
            dependenciesToPin
                .mapNotNull { (name, dep) -> buildDependencyCustomization(name, dep) }
                .toMap()

        // Return empty if no customizations needed
        return if (dependencyCustomizations.isEmpty()) {
            emptyMap()
        } else {
            mapOf("dependencies" to dependencyCustomizations)
        }
    }

    /**
     * Builds version and feature customizations for a dependency.
     * Returns null if there are no customizations to apply.
     */
    private fun buildDependencyCustomization(
        dependencyName: String,
        dependency: software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency,
    ): Pair<String, Map<String, Any>>? {
        val version = dependency.version()

        // Only customize if we have a concrete version (not "local")
        if (version == "local") {
            return null
        }

        val customizations = mutableMapOf<String, Any>("version" to version)

        // Include features if present
        if (dependency.features.isNotEmpty()) {
            customizations["features"] = dependency.features.toList()
        }

        return dependencyName to customizations
    }
}
