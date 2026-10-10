/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.protocols

import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType

/**
 * One representation of the `ClientProtocolSlot` that generated clients can read out of a
 * `ConfiguredProtocol`: a payload type from one compatibility line of a protocol-defining crate,
 * plus how to adapt it to the protocol trait this client calls.
 *
 * @property payloadType the payload type, e.g. `aws_smithy_schema::protocol::SchemaProtocol`. To
 * support a second compatibility line of the same crate, resolve it from an aliased dependency
 * (`CargoDependency(name = "aws-smithy-schema-v2", ..., package = "aws-smithy-schema")`) so that
 * both lines can be linked into the generated crate.
 * @property adapt renders an expression that turns `protocol: &PayloadType` into
 * `Result<SharedClientProtocol, E>` for this client's `SharedClientProtocol`, where
 * `E: Into<BoxError>`.
 */
internal data class ConfiguredProtocolRepresentation(
    val payloadType: RuntimeType,
    val adapt: Writable,
)

/**
 * The authoritative list of `ConfiguredProtocol` representations generated clients support.
 *
 * Generated clients enumerate this list instead of asking only the linked `aws-smithy-schema`
 * release to downcast the configured value, so a protocol configured through a different, but
 * supported, compatibility line is adapted rather than rejected. A value from any other line fails
 * the request with an error that names the configured representation and this list.
 *
 * When `aws-smithy-schema` (or another protocol-defining crate) starts a new compatibility line,
 * add an entry here with an aliased dependency and an adapter, or record an explicit decision not
 * to support it. Entries for lines that have left the support window may be removed.
 */
internal object ConfiguredProtocolRegistry {
    fun representations(runtimeConfig: RuntimeConfig): List<ConfiguredProtocolRepresentation> =
        listOf(
            // The compatibility line this client is generated against. `v1()` already adapts
            // between protocol-trait versions within the line.
            ConfiguredProtocolRepresentation(
                payloadType = RuntimeType.smithySchema(runtimeConfig).resolve("protocol::SchemaProtocol"),
                adapt = writable { rust("protocol.v1()") },
            ),
        )

    /**
     * Checks that [representations] can be linked into one generated crate and dispatched on:
     *
     * - there is at least one representation;
     * - no payload type is listed twice; and
     * - each dependency name has one version. A second compatibility line of a crate needs its own
     *   aliased dependency (`name = "aws-smithy-schema-v2"`, `package = "aws-smithy-schema"`);
     *   reusing the original name would make the generated manifest pick one version and drop the
     *   other.
     *
     * Payload types with no Cargo dependency (types local to the generated crate) are allowed.
     */
    fun validate(representations: List<ConfiguredProtocolRepresentation>) {
        check(representations.isNotEmpty()) { "at least one protocol representation must be supported" }

        val duplicatePaths =
            representations.groupBy { it.payloadType.fullyQualifiedName() }.filterValues { it.size > 1 }.keys
        check(duplicatePaths.isEmpty()) { "protocol representations are registered more than once: $duplicatePaths" }

        val dependencies = representations.mapNotNull { it.payloadType.dependency as? CargoDependency }
        dependencies.groupBy { it.name }.forEach { (name, deps) ->
            val versions = deps.map { it.version() }.distinct()
            check(versions.size == 1) {
                "dependency `$name` is registered at several versions $versions; give each compatibility line " +
                    "its own aliased dependency name"
            }
        }
    }
}

/**
 * Renders `resolve_client_protocol`, which generated serializers and deserializers call to obtain
 * the configured protocol.
 */
internal class ConfiguredProtocolResolver(
    private val runtimeConfig: RuntimeConfig,
    private val representations: List<ConfiguredProtocolRepresentation> =
        ConfiguredProtocolRegistry.representations(runtimeConfig),
) {
    init {
        ConfiguredProtocolRegistry.validate(representations)
    }

    private val runtimeApi = RuntimeType.smithyRuntimeApiClient(runtimeConfig)
    private val scope =
        arrayOf(
            *RuntimeType.preludeScope,
            "BoxError" to RuntimeType.boxError(runtimeConfig),
            "ConfigBag" to RuntimeType.configBag(runtimeConfig),
            "ClientProtocolSlot" to runtimeApi.resolve("client::protocol::ClientProtocolSlot"),
            "ConfiguredProtocol" to runtimeApi.resolve("client::protocol::ConfiguredProtocol"),
            "ConfigPayloadFor" to runtimeApi.resolve("client::versioned_config::ConfigPayloadFor"),
            "RepresentationId" to runtimeApi.resolve("client::versioned_config::RepresentationId"),
            "SharedClientProtocol" to RuntimeType.smithySchema(runtimeConfig).resolve("protocol::SharedClientProtocol"),
        )

    /** The generated `resolve_client_protocol(cfg)` function for the default registry. */
    fun resolveFn(): RuntimeType =
        RuntimeType.forInlineFun(FN_NAME, RustModule.private("protocol_resolution")) {
            render(this)
        }

    /** Renders `resolve_client_protocol` into [writer]. Exposed so tests can render custom registries. */
    fun render(writer: RustWriter) {
        val supported =
            writable {
                representations.forEach {
                    rustTemplate(
                        "<#{Payload} as #{ConfigPayloadFor}<#{ClientProtocolSlot}>>::REPRESENTATION,",
                        *scope,
                        "Payload" to it.payloadType,
                    )
                }
            }
        val branches =
            writable {
                representations.forEach {
                    rustTemplate(
                        """
                        if let #{Some}(protocol) = configured.downcast_ref::<#{Payload}>() {
                            return #{adapt}.map_err(#{Into}::into);
                        }
                        """,
                        *scope,
                        "Payload" to it.payloadType,
                        "adapt" to it.adapt,
                    )
                }
            }
        writer.rustTemplate(
            """
            /// Returns the configured client protocol, adapted to the protocol trait this client calls.
            ///
            /// Tries every `ConfiguredProtocol` representation this client was generated to support. A
            /// protocol from any other crate or compatibility line is an error naming both, never an
            /// absent setting.
            pub(crate) fn $FN_NAME(cfg: &#{ConfigBag}) -> #{Result}<#{SharedClientProtocol}, #{BoxError}> {
                static SUPPORTED: [#{RepresentationId}; ${representations.size}] = [#{supported}];
                let configured = cfg
                    .load::<#{ConfiguredProtocol}>()
                    .ok_or_else(#{ConfiguredProtocol}::missing_error)?;
                #{branches}
                #{Err}(configured.unsupported_error(&SUPPORTED).into())
            }
            """,
            *scope,
            "supported" to supported,
            "branches" to branches,
        )
    }

    companion object {
        const val FN_NAME = "resolve_client_protocol"
    }
}
