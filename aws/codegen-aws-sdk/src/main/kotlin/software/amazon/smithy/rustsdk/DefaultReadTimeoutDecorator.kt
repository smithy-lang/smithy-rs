/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rustsdk

import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginSection
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.customize.AdHocCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.adhocCustomization
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

/** Crate-private marker holding the resolved opt-in, so no public config surface is added. */
private const val OPT_IN_TYPE = "DefaultSocketTimeoutOptIn"

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
 * The whole thing is gated on [OPT_IN_ENV_VAR], which is resolved differently on the two
 * construction paths. From `SdkConfig` it comes through `aws-config`, which reads the environment
 * via an `Env` shim, so test utilities can isolate a client from the host environment and the
 * answer is fixed when the client is built. From a bare `Config::builder()` there is no `SdkConfig`
 * and so no `Env`, and the process environment is the only source available.
 *
 * Retiring the opt-in means deleting the gate, the marker type and the `SdkConfig` section;
 * retiring the feature means deleting the decorator.
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

    /** Emits the crate-private marker the opt-in is stored in. Nothing public is added. */
    override fun extras(
        codegenContext: ClientCodegenContext,
        rustCrate: RustCrate,
    ) {
        if (readTimeoutFor(codegenContext) == null) return
        val rc = codegenContext.runtimeConfig
        rustCrate.withModule(ClientRustModule.config) {
            rustTemplate(
                """
                /// Whether the caller opted in to the 2026 default socket timeouts.
                ///
                /// Resolved once, when the client is built, and then fixed for that client's
                /// lifetime. Absence is meaningful: see the comment where this is read.
                ##[derive(Debug, Clone)]
                pub(crate) struct $OPT_IN_TYPE(pub(crate) bool);
                impl #{Storable} for $OPT_IN_TYPE {
                    type Storer = #{StoreReplace}<Self>;
                }
                """,
                "Storable" to RuntimeType.smithyTypes(rc).resolve("config_bag::Storable"),
                "StoreReplace" to RuntimeType.smithyTypes(rc).resolve("config_bag::StoreReplace"),
            )
        }
    }

    /**
     * Resolves the opt-in through `aws-config` rather than from the process environment.
     *
     * Environment access is deliberately confined to `aws-config`, which reads through an `Env`
     * shim. Going through it means `ConfigLoader::empty_test_environment()` and
     * `ProviderConfig::with_env` isolate a client from the host environment, and that the answer is
     * fixed when the client is built rather than re-read later.
     */
    override fun extraSections(codegenContext: ClientCodegenContext): List<AdHocCustomization> {
        if (readTimeoutFor(codegenContext) == null) return emptyList()
        val serviceId = codegenContext.serviceShape.sdkId()
        return listOf(
            adhocCustomization<SdkConfigSection.CopySdkConfigToClientConfig> { section ->
                rustTemplate(
                    """
                    // Stored unconditionally, including `false`. An absent value then means "this
                    // client was not built from `SdkConfig`" and never "the variable was unset",
                    // which is what lets the fallback below stay off the host environment for a
                    // client built with `empty_test_environment()`.
                    ${section.serviceConfigBuilder}.config.store_put(
                        crate::config::$OPT_IN_TYPE(
                            ${section.sdkConfig}
                                .service_config()
                                .and_then(|conf| {
                                    // Empty profile key: the program specifies this opt-in as an
                                    // environment variable only, so shared config is not a source.
                                    conf.load_config(service_config_key(
                                        ${serviceId.dq()},
                                        ${OPT_IN_ENV_VAR.dq()},
                                        "",
                                    ))
                                })
                                .map(|it| it.eq_ignore_ascii_case("true"))
                                .unwrap_or_default(),
                        ),
                    );
                    """,
                )
            },
        )
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
                    // The opt-in for the 2026 default socket timeouts, resolved when this client
                    // was built.
                    //
                    // A client built from `SdkConfig` always stored an answer, resolved through
                    // `aws-config` so that `Env`-based test utilities isolate it from the host
                    // environment. So `None` here means the client came straight from
                    // `Config::builder()`, where there is no `SdkConfig` and therefore no `Env` to
                    // consult; the process environment is the only source available on that path.
                    //
                    // Only a case-insensitive "true" enables the default. Any other value leaves it
                    // off rather than failing, so a typo cannot stop a client from being built.
                    let opt_in = ${section.serviceConfigName}
                        .config
                        .load::<crate::config::$OPT_IN_TYPE>()
                        .map(|it| it.0)
                        .unwrap_or_else(|| {
                            ::std::env::var(${OPT_IN_ENV_VAR.dq()})
                                .map(|v| v.eq_ignore_ascii_case("true"))
                                .unwrap_or_default()
                        });
                    if opt_in {
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
