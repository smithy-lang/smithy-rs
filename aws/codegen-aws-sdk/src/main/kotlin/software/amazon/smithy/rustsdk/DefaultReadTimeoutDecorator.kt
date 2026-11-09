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
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.sdkId
import java.time.Duration
import kotlin.io.path.readText

/**
 * The opt-in that turns the default read timeout on.
 *
 * Public distributions ship the default off. A caller opts in by setting this to a truthy value;
 * the program retires the variable when the default flips on unconditionally, so it is deliberately
 * not reachable from client code — nothing has to be supported past that point.
 */
private const val OPT_IN_ENV_VAR = "AWS_ENABLE_DEFAULT_SOCKET_TIMEOUT_2026"

/**
 * Applies a default read timeout per service, from the shared timeout-defaults artifact.
 *
 * The read timeout bounds request dispatch until response headers arrive. Without one, a peer that
 * accepts a connection and then goes silent leaves the request hanging until the OS gives up.
 *
 * The value is resolved at codegen time by [TimeoutDefaults] and emitted into the generated
 * `ServiceRuntimePlugin`'s config layer. Three things make that the right place:
 *
 * - the layer is `Order::Defaults`, below the caller's `Config` layer, so any `read_timeout` the
 *   caller sets — longer, shorter, or disabled, on the client or per operation — wins
 * - `TimeoutConfig` merges per field across layers, so this does not disturb the connect timeout
 * - `ServiceRuntimePlugin::new` runs on both construction paths, so one hook covers both
 *   `Config::from(&sdk_config)` and `Config::builder()`
 *
 * `rust-runtime/aws-smithy-runtime/tests/timeout_config_layer_precedence.rs` pins all three.
 *
 * The whole thing is gated on [OPT_IN_ENV_VAR], read at client construction. Retiring the opt-in
 * later means deleting the `if` and this comment; retiring the feature means deleting the decorator.
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

    override fun serviceRuntimePluginCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ServiceRuntimePluginCustomization>,
    ): List<ServiceRuntimePluginCustomization> {
        val readTimeout =
            timeoutDefaults(codegenContext)?.readTimeoutFor(codegenContext.serviceShape.sdkId())
                ?: return baseCustomizations
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
                    // Opt-in for the 2026 default socket timeouts. Only a case-insensitive "true"
                    // enables it; any other value is ignored rather than rejected, so a typo cannot
                    // stop a client from being built. Read per client construction, not cached, so
                    // a process that changes the variable is not stuck with the first answer.
                    if ::std::env::var(${OPT_IN_ENV_VAR.dq()})
                        .map(|v| v.eq_ignore_ascii_case("true"))
                        .unwrap_or_default()
                    {
                        ${section.newLayerName}.store_put(
                            #{TimeoutConfig}::builder()
                                .read_timeout(#{Duration}::from_millis(${readTimeout.toMillis()}))
                                .build(),
                        );
                    }
                    """,
                    "TimeoutConfig" to RuntimeType.smithyTypes(rc).resolve("timeout::TimeoutConfig"),
                    "Duration" to RuntimeType.Duration,
                )
            }
        }
}
