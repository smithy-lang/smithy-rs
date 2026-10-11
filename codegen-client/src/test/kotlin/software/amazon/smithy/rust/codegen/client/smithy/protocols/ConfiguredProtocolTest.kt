/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.protocols

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customizations.SchemaSerdeAllowlist
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest

/**
 * A generated client's protocol setting is stored as the version-stable `ConfiguredProtocol` from
 * `aws-smithy-runtime-api`, not as a type from `aws-smithy-schema`, so that the setting survives a
 * major version of `aws-smithy-schema`.
 *
 * These tests pin the two halves of that contract on a generated client:
 *
 * - a `ConfiguredProtocol` passed to `set_protocol` (what `SdkConfig` carries) is honored; and
 * - a `ConfiguredProtocol` wrapping a handle the client cannot use — standing in for a protocol built
 *   against a different major version of `aws-smithy-schema` — fails the request with an error naming
 *   both versions, rather than panicking or silently falling back to the default protocol.
 */
class ConfiguredProtocolTest {
    private val model =
        """
        namespace smithy.rust.codegen.test.schemaheaders

        @aws.protocols#awsJson1_0
        service ConfiguredProtocolService {
            version: "2024-01-01",
            operations: [GetStats]
        }

        operation GetStats {
            input := { name: String }
            output := { value: String }
        }
        """.asSmithyModel(smithyVersion = "2.0")

    @Test
    fun `configured protocol is honored and a foreign one is a descriptive error`() {
        clientIntegrationTest(model) { context: ClientCodegenContext, rustCrate ->
            check(SchemaSerdeAllowlist.usesSchemaSerdeExclusively(context)) {
                "the dedicated fixture namespace must exercise the schema-exclusive path"
            }
            val scope =
                arrayOf(
                    *RuntimeType.preludeScope,
                    "capture_request" to RuntimeType.captureRequest(context.runtimeConfig),
                    "SharedClientProtocol" to
                        RuntimeType.smithySchema(context.runtimeConfig).resolve("protocol::SharedClientProtocol"),
                    "ConfiguredProtocol" to
                        RuntimeType.smithyRuntimeApiClient(context.runtimeConfig)
                            .resolve("client::protocol::ConfiguredProtocol"),
                    "ProtocolHandle" to
                        RuntimeType.smithyRuntimeApiClient(context.runtimeConfig)
                            .resolve("client::protocol::ProtocolHandle"),
                    "ClientProtocolSlot" to
                        RuntimeType.smithyRuntimeApiClient(context.runtimeConfig)
                            .resolve("client::protocol::ClientProtocolSlot"),
                    "ConfigPayloadFor" to
                        RuntimeType.smithyRuntimeApiClient(context.runtimeConfig)
                            .resolve("client::versioned_config::ConfigPayloadFor"),
                    "RepresentationId" to
                        RuntimeType.smithyRuntimeApiClient(context.runtimeConfig)
                            .resolve("client::versioned_config::RepresentationId"),
                    "AwsRestJsonProtocol" to
                        RuntimeType.smithyJson(context.runtimeConfig)
                            .resolve("protocol::aws_rest_json_1::AwsRestJsonProtocol"),
                    "HttpRequest" to
                        RuntimeType.smithyRuntimeApiClient(context.runtimeConfig)
                            .resolve("client::orchestrator::HttpRequest"),
                    "Endpoint" to RuntimeType.smithyTypes(context.runtimeConfig).resolve("endpoint::Endpoint"),
                    "ConfigBag" to RuntimeType.configBag(context.runtimeConfig),
                    "BoxError" to RuntimeType.boxError(context.runtimeConfig),
                    "DisplayErrorContext" to
                        RuntimeType.smithyTypes(context.runtimeConfig).resolve("error::display::DisplayErrorContext"),
                )
            rustCrate.testModule {
                tokioTest("set_protocol_accepts_a_configured_protocol") {
                    rustTemplate(
                        """
                        let (http_client, rx) = #{capture_request}(#{None});
                        let mut builder = crate::Config::builder()
                            .http_client(http_client)
                            .endpoint_url("http://localhost:1234")
                            .behavior_version_latest();
                        // What `SdkConfigDecorator` emits when `SdkConfig::protocol` is set.
                        builder.set_protocol(#{Some}(#{SharedClientProtocol}::configured(
                            #{AwsRestJsonProtocol}::new(),
                        )));
                        let config = builder.build();
                        let representation = config.protocol().expect("set").representation();
                        assert_eq!("aws-smithy-schema", representation.package());
                        assert_eq!("1", representation.compatibility_line());
                        let client = crate::Client::from_conf(config);

                        let _ = client.get_stats().name("test").send().await;
                        let request = rx.expect_request();
                        assert_eq!(
                            #{Some}("application/json"),
                            request.headers().get("Content-Type"),
                            "the configured restJson1 protocol, not the generated awsJson1_0 one, must serialize",
                        );
                        """,
                        *scope,
                    )
                }

                tokioTest("protocol_from_another_schema_major_version_fails_the_request") {
                    rustTemplate(
                        """
                        ##[derive(Debug)]
                        struct FutureSchemaProtocol;
                        impl #{ConfigPayloadFor}<#{ClientProtocolSlot}> for FutureSchemaProtocol {
                            const REPRESENTATION: #{RepresentationId} =
                                #{RepresentationId}::new("aws-smithy-schema", "2", 1);
                        }
                        impl #{ProtocolHandle} for FutureSchemaProtocol {
                            fn update_endpoint(
                                &self,
                                _request: &mut #{HttpRequest},
                                _endpoint: &#{Endpoint},
                                _cfg: &#{ConfigBag},
                            ) -> #{Result}<(), #{BoxError}> {
                                #{Ok}(())
                            }
                        }

                        let (http_client, rx) = #{capture_request}(#{None});
                        let mut builder = crate::Config::builder()
                            .http_client(http_client)
                            .endpoint_url("http://localhost:1234")
                            .behavior_version_latest();
                        builder.set_protocol(#{Some}(#{ConfiguredProtocol}::new(FutureSchemaProtocol)));
                        let client = crate::Client::from_conf(builder.build());

                        let err = client
                            .get_stats()
                            .name("test")
                            .send()
                            .await
                            .expect_err("a protocol this client cannot use must fail the request");
                        let message = format!("{}", #{DisplayErrorContext}(&err));
                        assert!(
                            message.contains("built with aws-smithy-schema@2 (api revision 1)")
                                && message.contains("this client supports [aws-smithy-schema@1 (api revision 1)]"),
                            "{message}",
                        );
                        // Serialization failed, so nothing was sent.
                        rx.expect_no_request();
                        """,
                        *scope,
                    )
                }
            }
        }
    }

    /**
     * The generated resolver tries every registered representation. A second entry stands in for a
     * future `aws-smithy-schema` compatibility line: a distinct payload type with its own
     * representation and an adapter to this client's `SharedClientProtocol`.
     */
    @Test
    fun `resolver adapts every registered representation and rejects others`() {
        clientIntegrationTest(model) { context: ClientCodegenContext, rustCrate ->
            val rc = context.runtimeConfig
            val runtimeApi = RuntimeType.smithyRuntimeApiClient(rc)
            val fakeModule = RustModule.pubCrate("fake_schema_v2").cfgTest()
            val resolverModule = RustModule.pubCrate("multi_line_resolution").cfgTest()
            val scope =
                arrayOf(
                    *RuntimeType.preludeScope,
                    "ProtocolHandle" to runtimeApi.resolve("client::protocol::ProtocolHandle"),
                    "ClientProtocolSlot" to runtimeApi.resolve("client::protocol::ClientProtocolSlot"),
                    "ConfiguredProtocol" to runtimeApi.resolve("client::protocol::ConfiguredProtocol"),
                    "ConfigPayloadFor" to runtimeApi.resolve("client::versioned_config::ConfigPayloadFor"),
                    "RepresentationId" to runtimeApi.resolve("client::versioned_config::RepresentationId"),
                    "ConfigSlotError" to runtimeApi.resolve("client::versioned_config::ConfigSlotError"),
                    "HttpRequest" to runtimeApi.resolve("client::orchestrator::HttpRequest"),
                    "Endpoint" to RuntimeType.smithyTypes(rc).resolve("endpoint::Endpoint"),
                    "ConfigBag" to RuntimeType.configBag(rc),
                    "Layer" to RuntimeType.smithyTypes(rc).resolve("config_bag::Layer"),
                    "BoxError" to RuntimeType.boxError(rc),
                    "SharedClientProtocol" to RuntimeType.smithySchema(rc).resolve("protocol::SharedClientProtocol"),
                    "AwsRestJsonProtocol" to
                        RuntimeType.smithyJson(rc).resolve("protocol::aws_rest_json_1::AwsRestJsonProtocol"),
                )
            rustCrate.withModule(fakeModule) {
                rustTemplate(
                    """
                    /// Stands in for `aws_smithy_schema_v2::protocol::SchemaProtocol`.
                    ##[derive(Debug)]
                    pub(crate) struct FakeSchemaV2Protocol;
                    impl #{ConfigPayloadFor}<#{ClientProtocolSlot}> for FakeSchemaV2Protocol {
                        const REPRESENTATION: #{RepresentationId} =
                            #{RepresentationId}::new("aws-smithy-schema", "2", 1);
                    }
                    /// A line nobody registered.
                    ##[derive(Debug)]
                    pub(crate) struct UnregisteredProtocol;
                    impl #{ConfigPayloadFor}<#{ClientProtocolSlot}> for UnregisteredProtocol {
                        const REPRESENTATION: #{RepresentationId} =
                            #{RepresentationId}::new("aws-smithy-schema", "3", 1);
                    }
                    macro_rules! no_op_handle {
                        (${'$'}t:ty) => {
                            impl #{ProtocolHandle} for ${'$'}t {
                                fn update_endpoint(
                                    &self,
                                    _request: &mut #{HttpRequest},
                                    _endpoint: &#{Endpoint},
                                    _cfg: &#{ConfigBag},
                                ) -> #{Result}<(), #{BoxError}> {
                                    #{Ok}(())
                                }
                            }
                        };
                    }
                    no_op_handle!(FakeSchemaV2Protocol);
                    no_op_handle!(UnregisteredProtocol);

                    /// Adapter from the fake line to this client's protocol trait.
                    pub(crate) fn adapt(_protocol: &FakeSchemaV2Protocol) -> #{Result}<#{SharedClientProtocol}, #{BoxError}> {
                        #{Ok}(#{SharedClientProtocol}::new(#{AwsRestJsonProtocol}::new()))
                    }
                    """,
                    *scope,
                )
            }
            val representations =
                ConfiguredProtocolRegistry.representations(rc) +
                    ConfiguredProtocolRepresentation(
                        payloadType = RuntimeType("crate::fake_schema_v2::FakeSchemaV2Protocol"),
                        adapt = writable { rust("crate::fake_schema_v2::adapt(protocol)") },
                    )
            rustCrate.withModule(resolverModule) {
                ConfiguredProtocolResolver(rc, representations).render(this)
            }
            rustCrate.testModule {
                tokioTest("resolver_handles_each_registered_line") {
                    rustTemplate(
                        """
                        use crate::multi_line_resolution::resolve_client_protocol;
                        use crate::fake_schema_v2::{FakeSchemaV2Protocol, UnregisteredProtocol};
                        fn bag(protocol: #{ConfiguredProtocol}) -> #{ConfigBag} {
                            let mut layer = #{Layer}::new("test");
                            layer.store_put(protocol);
                            #{ConfigBag}::of_layers(vec![layer])
                        }

                        // This client's own line.
                        let own = bag(#{SharedClientProtocol}::configured(#{AwsRestJsonProtocol}::new()));
                        assert!(resolve_client_protocol(&own).is_ok());

                        // A different, registered line is adapted.
                        let v2 = bag(#{ConfiguredProtocol}::new(FakeSchemaV2Protocol));
                        let adapted = resolve_client_protocol(&v2).expect("v2 is registered");
                        assert_eq!("aws.protocols##restJson1", adapted.protocol_id().as_str());

                        // An unregistered line names itself and every supported line.
                        let v3 = bag(#{ConfiguredProtocol}::new(UnregisteredProtocol));
                        let err = resolve_client_protocol(&v3).expect_err("v3 is not registered");
                        let slot = err.downcast_ref::<#{ConfigSlotError}>().expect("slot error");
                        assert!(slot.is_unsupported());
                        let message = err.to_string();
                        assert!(message.contains("built with aws-smithy-schema@3"), "{message}");
                        assert!(
                            message.contains("[aws-smithy-schema@1 (api revision 1), aws-smithy-schema@2 (api revision 1)]"),
                            "{message}",
                        );

                        // Nothing configured.
                        let err = resolve_client_protocol(&#{ConfigBag}::base()).expect_err("missing");
                        assert!(err.downcast_ref::<#{ConfigSlotError}>().unwrap().is_missing());
                        """,
                        *scope,
                    )
                }
            }
        }
    }
}
