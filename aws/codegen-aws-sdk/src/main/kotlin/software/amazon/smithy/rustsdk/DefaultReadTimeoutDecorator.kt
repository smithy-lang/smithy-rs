/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rustsdk

import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginSection
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.util.PANIC
import software.amazon.smithy.rust.codegen.core.util.sdkId
import java.time.Duration
import kotlin.io.path.readText

/** The behavior version that turns the default read timeout on. */
private const val BEHAVIOR_VERSION = "v2026_11_17"

/**
 * Applies a default read timeout per service, from the shared timeout-defaults artifact.
 *
 * The read timeout bounds request dispatch until response headers arrive. Without one, a peer that
 * accepts a connection and then goes silent leaves the request hanging until the OS gives up.
 *
 * [TimeoutDefaults] resolves the value at codegen time, and it is emitted into the generated
 * `ServiceRuntimePlugin`'s config layer. Three properties of that layer make it the right place,
 * all pinned by `aws-smithy-runtime/tests/timeout_config_layer_precedence.rs`:
 *
 * - it is `Order::Defaults`, below the caller's `Config` layer, so any `read_timeout` the caller
 *   sets — longer, shorter, or disabled, on the client or per operation — wins
 * - `TimeoutConfig` merges per field across layers, so this does not disturb the connect timeout
 * - `ServiceRuntimePlugin::new` runs on both construction paths, so one hook covers both
 *   `Config::from(&sdk_config)` and `Config::builder()`
 *
 * Gating here rather than in the runtime's `default_plugins` is deliberate: a runtime-side gate
 * would hand new behavior to anyone who bumps `aws-smithy-runtime` alone, which `cargo update` can
 * do without touching an SDK crate.
 */
class DefaultReadTimeoutDecorator : ClientCodegenDecorator {
    override val name: String = "DefaultReadTimeout"
    override val order: Byte = 0

    // The decorator instance is reused across services, so parse the artifact once.
    private var cache: TimeoutDefaults? = null

    private fun timeoutDefaults(codegenContext: ClientCodegenContext): TimeoutDefaults? {
        if (cache == null) {
            val sdkSettings = SdkSettings.from(codegenContext.settings)
            val path =
                sdkSettings.timeoutDefaultsConfigPath
                    ?: if (sdkSettings.awsSdkBuild) {
                        PANIC("`timeoutDefaultsConfigPath` is required in an AWS SDK build")
                    } else {
                        // A non-SDK build has no artifact and gets no AWS timeout defaults.
                        return null
                    }
            cache = TimeoutDefaults(Node.parse(path.readText()).expectObjectNode() as ObjectNode)
        }
        return cache
    }

    /** The service's default, or null when it is fully exempt and nothing should be emitted. */
    private fun readTimeoutFor(codegenContext: ClientCodegenContext): Duration? =
        timeoutDefaults(codegenContext)?.readTimeoutFor(codegenContext.serviceShape.sdkId())

    override fun serviceRuntimePluginCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ServiceRuntimePluginCustomization>,
    ): List<ServiceRuntimePluginCustomization> {
        val readTimeout = readTimeoutFor(codegenContext) ?: return baseCustomizations
        return baseCustomizations + DefaultReadTimeoutCustomization(codegenContext, readTimeout)
    }
}

/**
 * Emits the per-service default into the service runtime plugin's config layer.
 *
 * Only constructed for a service that has a value. A fully exempt service contributes nothing at all,
 * which matters: an empty `TimeoutConfig` is not neutral. `MergeTimeoutConfig::merge_iter` stops
 * absorbing lower-priority layers once the accumulated value has no field in the `Set` state, so an
 * empty entry here would truncate the merge chain and take the connect-timeout default with it.
 */
private class DefaultReadTimeoutCustomization(
    private val codegenContext: ClientCodegenContext,
    private val readTimeout: Duration,
) : ServiceRuntimePluginCustomization() {
    override fun section(section: ServiceRuntimePluginSection): Writable =
        writable {
            if (section is ServiceRuntimePluginSection.AdditionalConfig) {
                val rc = codegenContext.runtimeConfig
                rustTemplate(
                    """
                    if ${section.serviceConfigName}
                        .behavior_version
                        .is_some_and(|bv| bv.is_at_least(#{BehaviorVersion}::$BEHAVIOR_VERSION()))
                    {
                        ${section.newLayerName}.store_put(
                            #{TimeoutConfig}::builder()
                                .read_timeout(#{Duration}::from_millis(${readTimeout.toMillis()}))
                                .build(),
                        );
                    }
                    """,
                    "BehaviorVersion" to
                        RuntimeType.smithyRuntimeApiClient(rc)
                            .resolve("client::behavior_version::BehaviorVersion"),
                    "TimeoutConfig" to RuntimeType.smithyTypes(rc).resolve("timeout::TimeoutConfig"),
                    "Duration" to RuntimeType.Duration,
                )
            }
        }
}
