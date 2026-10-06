/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.protocols

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customizations.SchemaSerdeAllowlist
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
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
                        assert_eq!(
                            config.protocol().map(|p| p.origin()),
                            #{Some}(concat!("aws-smithy-schema ", "1.x")),
                        );
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
                        impl #{ProtocolHandle} for FutureSchemaProtocol {
                            fn origin(&self) -> &'static str {
                                "aws-smithy-schema 2.x"
                            }
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
                            message.contains("built for aws-smithy-schema 2.x")
                                && message.contains("requires a protocol built for aws-smithy-schema 1.x"),
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
}
