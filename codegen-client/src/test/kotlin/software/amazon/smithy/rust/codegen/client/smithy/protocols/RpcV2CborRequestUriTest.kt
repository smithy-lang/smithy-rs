/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.protocols

import org.junit.jupiter.api.Assumptions.assumeTrue
import org.junit.jupiter.api.Test
import software.amazon.smithy.aws.traits.protocols.AwsJson1_0Trait
import software.amazon.smithy.protocol.traits.Rpcv2CborTrait
import software.amazon.smithy.rust.codegen.client.smithy.customizations.SchemaSerdeAllowlist
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import kotlin.io.path.readText

/**
 * Regression tests for https://github.com/smithy-lang/smithy-rs/issues/4801.
 *
 * `rpcv2Cbor` ignores `@http` bindings and always routes to
 * `/service/{serviceName}/operation/{operationName}`. A model may nevertheless
 * carry `@http` traits on its operations (they are simply inert for this
 * protocol) — the Pokémon example model in this repo is one such model.
 *
 * The schema-serde request path used to decide whether to pass a URI path to
 * `ClientProtocol::serialize_request` by asking whether the *operation* had an
 * `@http` trait, rather than whether the *protocol* honors HTTP bindings. For
 * `rpcv2Cbor` operations carrying `@http`, that passed an empty endpoint, and
 * `HttpRpcProtocol::serialize_request` falls back to `/` when the endpoint is
 * empty — so requests were sent to `/` and servers answered 404.
 */
internal class RpcV2CborRequestUriTest {
    /** Operations carry inert `@http` traits, as the Pokémon model does. */
    private val modelWithHttpTraits =
        """
        ${'$'}version: "2"
        namespace test

        use smithy.protocols#rpcv2Cbor

        @rpcv2Cbor
        service RpcV2CborWithHttp {
            version: "2019-12-16",
            operations: [GetStats, EmptyInputOp]
        }

        @http(uri: "/stats", method: "PUT")
        operation GetStats {
            input: GetStatsInput,
            output: GetStatsOutput
        }

        structure GetStatsInput {
            name: String
        }

        structure GetStatsOutput {
            callsCount: Long
        }

        // Per the rpcv2Cbor spec, operations with no modeled input send no body.
        // This exercises the separate "no content type" branch of the generator.
        @http(uri: "/empty", method: "POST")
        operation EmptyInputOp {
            output: GetStatsOutput
        }
        """.asSmithyModel()

    /** Same service, but without any `@http` traits — the previously-working case. */
    private val modelWithoutHttpTraits =
        """
        ${'$'}version: "2"
        namespace test

        use smithy.protocols#rpcv2Cbor

        @rpcv2Cbor
        service RpcV2CborNoHttp {
            version: "2019-12-16",
            operations: [GetStats]
        }

        @readonly
        operation GetStats {
            input: GetStatsInput,
            output: GetStatsOutput
        }

        structure GetStatsInput {
            name: String
        }

        structure GetStatsOutput {
            callsCount: Long
        }
        """.asSmithyModel()

    /**
     * A service generated for a *different* protocol. `rpcv2Cbor` can be plugged into it at
     * runtime via `Config::builder().protocol(..)` — SEP goal (2), which exists so customers can
     * pin or migrate protocols without regenerating. The route is therefore not something codegen
     * can decide: the client here was generated with awsJson1_0's `/` baked in.
     */
    private val awsJsonModel =
        """
        ${'$'}version: "2"
        namespace test

        use aws.protocols#awsJson1_0

        @awsJson1_0
        service SwapTargetService {
            version: "2019-12-16",
            operations: [GetStats]
        }

        @optionalAuth
        operation GetStats {
            input: GetStatsInput,
            output: GetStatsOutput
        }

        structure GetStatsInput {
            name: String
        }

        structure GetStatsOutput {
            callsCount: Long
        }
        """.asSmithyModel()

    @Test
    fun `rpcv2Cbor uses the canonical RPC route even when operations carry @http traits`() {
        assumeTrue(
            SchemaSerdeAllowlist.isProtocolEnabled(Rpcv2CborTrait.ID),
            "rpcv2Cbor is not on SchemaSerdeAllowlist, so the schema-serde request path is not generated",
        )
        val testDir =
            clientIntegrationTest(modelWithHttpTraits) { context, rustCrate ->
                rustCrate.testModule {
                    tokioTest("http_trait_does_not_override_rpc_route") {
                        rustTemplate(
                            """
                            let (http_client, rx) = #{capture_request}(#{None});
                            let config = crate::Config::builder()
                                .http_client(http_client)
                                .endpoint_url("http://localhost:1234")
                                .behavior_version_latest()
                                .build();
                            let client = crate::Client::from_conf(config);

                            let _ = client.get_stats().name("test").send().await;

                            let request = rx.expect_request();
                            assert_eq!(
                                "http://localhost:1234/service/RpcV2CborWithHttp/operation/GetStats",
                                request.uri(),
                                "rpcv2Cbor must route to the canonical RPC path, not the inert @http URI or `/`",
                            );
                            // The inert `@http(method: "PUT")` must not leak either.
                            assert_eq!("POST", request.method());
                            """,
                            *RuntimeType.preludeScope,
                            "capture_request" to RuntimeType.captureRequest(context.runtimeConfig),
                        )
                    }

                    tokioTest("http_trait_does_not_override_rpc_route_for_empty_input") {
                        rustTemplate(
                            """
                            let (http_client, rx) = #{capture_request}(#{None});
                            let config = crate::Config::builder()
                                .http_client(http_client)
                                .endpoint_url("http://localhost:1234")
                                .behavior_version_latest()
                                .build();
                            let client = crate::Client::from_conf(config);

                            let _ = client.empty_input_op().send().await;

                            let request = rx.expect_request();
                            assert_eq!(
                                "http://localhost:1234/service/RpcV2CborWithHttp/operation/EmptyInputOp",
                                request.uri(),
                            );
                            """,
                            *RuntimeType.preludeScope,
                            "capture_request" to RuntimeType.captureRequest(context.runtimeConfig),
                        )
                    }
                }
            }

        // The runtime protocol re-resolves the route, so the assertions above hold even if codegen
        // passes the wrong path. Pin the codegen layer separately: it must hand the resolved RPC
        // route to `serialize_request`, not the empty string that means "REST protocol — resolve
        // the `@http` template yourself".
        val serializer = testDir.resolve("src/operation/get_stats.rs").readText()
        assert(serializer.contains("\"/service/RpcV2CborWithHttp/operation/GetStats\"")) {
            "Expected the generated request serializer to pass the canonical RPC route to " +
                "`serialize_request`. Generated source:\n$serializer"
        }
    }

    @Test
    fun `rpcv2Cbor uses the canonical RPC route without @http traits`() {
        assumeTrue(
            SchemaSerdeAllowlist.isProtocolEnabled(Rpcv2CborTrait.ID),
            "rpcv2Cbor is not on SchemaSerdeAllowlist, so the schema-serde request path is not generated",
        )
        clientIntegrationTest(modelWithoutHttpTraits) { context, rustCrate ->
            rustCrate.testModule {
                tokioTest("rpc_route_without_http_trait") {
                    rustTemplate(
                        """
                        let (http_client, rx) = #{capture_request}(#{None});
                        let config = crate::Config::builder()
                            .http_client(http_client)
                            .endpoint_url("http://localhost:1234")
                            .behavior_version_latest()
                            .build();
                        let client = crate::Client::from_conf(config);

                        let _ = client.get_stats().name("test").send().await;

                        let request = rx.expect_request();
                        assert_eq!(
                            "http://localhost:1234/service/RpcV2CborNoHttp/operation/GetStats",
                            request.uri(),
                        );
                        """,
                        *RuntimeType.preludeScope,
                        "capture_request" to RuntimeType.captureRequest(context.runtimeConfig),
                    )
                }
            }
        }
    }

    /**
     * The route must also be correct when `rpcv2Cbor` is selected at *runtime* on a client
     * generated for another protocol, since that is the protocol-migration path the SEP's runtime
     * protocol selection exists to support. Codegen cannot resolve the route in that case — it
     * baked in awsJson1_0's `/` — so the runtime protocol has to resolve it from the config bag.
     */
    @Test
    fun `rpcv2Cbor plugged in at runtime uses the canonical RPC route`() {
        assumeTrue(
            SchemaSerdeAllowlist.isProtocolEnabled(AwsJson1_0Trait.ID),
            "awsJson1_0 is not on SchemaSerdeAllowlist, so there is no protocol to swap out",
        )
        clientIntegrationTest(awsJsonModel) { context, rustCrate ->
            rustCrate.testModule {
                tokioTest("runtime_protocol_swap_uses_rpc_route") {
                    rustTemplate(
                        """
                        let (http_client, rx) = #{capture_request}(#{None});
                        let config = crate::Config::builder()
                            .http_client(http_client)
                            .endpoint_url("http://localhost:1234")
                            .behavior_version_latest()
                            // Swap awsJson1_0 out for rpcv2Cbor at runtime.
                            .protocol(#{RpcV2CborProtocol}::new())
                            .build();
                        let client = crate::Client::from_conf(config);

                        let _ = client.get_stats().name("test").send().await;

                        let request = rx.expect_request();
                        assert_eq!(
                            "http://localhost:1234/service/SwapTargetService/operation/GetStats",
                            request.uri(),
                            "a runtime-selected rpcv2Cbor protocol must resolve its own route",
                        );
                        assert_eq!("POST", request.method());
                        assert_eq!(
                            #{Some}("application/cbor"),
                            request.headers().get("Content-Type"),
                        );
                        """,
                        *RuntimeType.preludeScope,
                        "capture_request" to RuntimeType.captureRequest(context.runtimeConfig),
                        "RpcV2CborProtocol" to
                            RuntimeType.smithyCbor(context.runtimeConfig).resolve("protocol::RpcV2CborProtocol"),
                    )
                }
            }
        }
    }
}
