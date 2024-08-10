/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rustsdk

import SdkCodegenIntegrationTest
import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest

class SigV4AuthDecoratorTest {
    private val modelWithSigV4AuthScheme =
        """
        namespace test

        use aws.auth#sigv4
        use aws.api#service
        use aws.protocols#restJson1
        use smithy.rules#endpointRuleSet
        use aws.auth#unsignedPayload
        use smithy.test#httpRequestTests

        @auth([sigv4])
        @sigv4(name: "dontcare")
        @restJson1
        @endpointRuleSet({
            "version": "1.0",
            "rules": [{ "type": "endpoint", "conditions": [], "endpoint": { "url": "https://example.com" } }],
            "parameters": {
                "endpoint": { "required": true, "type": "string", "builtIn": "SDK::Endpoint" },
            }
        })
        @service(sdkId: "dontcare")
        service TestService { version: "2023-01-01", operations: [SomeOperation] }
        structure SomeOutput { something: String }

        structure SomeInput {
            @httpPayload
            something: Bytestream
         }

        @streaming
        blob Bytestream

        @httpRequestTests([{
            id: "unsignedPayload",
            protocol: restJson1,
            method: "POST",
            uri: "/",
            params: {
                something: "hello"
            },
            headers: {
                "x-amz-content-sha256": "UNSIGNED-PAYLOAD",
            },
        }])
        @unsignedPayload
        @http(uri: "/", method: "POST")
        operation SomeOperation { input: SomeInput, output: SomeOutput }
        """.asSmithyModel()

    @Test
    fun unsignedPayloadSetsCorrectHeader() {
        awsSdkIntegrationTest(modelWithSigV4AuthScheme) { _, _ -> }
    }

    @Test
    fun authorizationHeader() {
        awsSdkIntegrationTest(SdkCodegenIntegrationTest.model) { ctx, rustCrate ->
            rustCrate.integrationTest("authorization_header") {
                tokioTest("test_signature") {
                    val rc = ctx.runtimeConfig
                    val moduleName = ctx.moduleUseName()
                    rustTemplate(
                        """
                        use $moduleName::config::{
                            Credentials, FrozenLayer, Layer, Region, RuntimeComponentsBuilder, RuntimePlugin,
                            SharedCredentialsProvider
                        };
                        use $moduleName::{Client, Config};

                        ##[derive(Debug)]
                        struct DeterministicAuthorizationHeaderPlugin(RuntimeComponentsBuilder);

                        impl DeterministicAuthorizationHeaderPlugin {
                            fn new() -> Self {
                                Self(RuntimeComponentsBuilder::new("DeterministicAuthorizationHeaderPlugin"))
                            }
                        }

                        // Exclude user agent headers, specifically `x-amz-user-agent`, to ensure the `authorization`
                        // header is deterministic.
                        // If we encounter signed headers that cause the `authorization` header to become
                        // nondeterministic in the future, we will disable the corresponding interceptors here to ensure
                        // the test remains valid. This plugin serves as a reference for managing the list of
                        // interceptors that impact the determinism of the `authorization` header.
                        impl RuntimePlugin for DeterministicAuthorizationHeaderPlugin {
                            fn config(&self) -> Option<FrozenLayer> {
                                let mut layer = Layer::new("DeterministicAuthorizationHeaderPlugin");
                                layer.store_put(#{disable_interceptor}::<#{UserAgentInterceptor}>(
                                    "DeterministicAuthorizationHeader",
                                ));
                                Some(layer.freeze())
                            }

                            fn runtime_components(
                                &self,
                                _: &RuntimeComponentsBuilder,
                            ) -> std::borrow::Cow<'_, RuntimeComponentsBuilder> {
                                std::borrow::Cow::Borrowed(&self.0)
                            }
                        }

                        let (http_client, rcvr) = #{capture_request}(#{None});
                        let config = Config::builder()
                            .credentials_provider(SharedCredentialsProvider::new(Credentials::for_tests()))
                            .region(Region::new("us-east-1"))
                            .http_client(http_client.clone())
                            .runtime_plugin(DeterministicAuthorizationHeaderPlugin::new())
                            .with_test_defaults()
                            .build();

                        let client = Client::from_conf(config);
                        let _ = client.some_operation().send().await;

                        let expected_req = rcvr.expect_request();
                        let auth_header = expected_req
                            .headers()
                            .get("authorization")
                            .unwrap()
                            .to_owned();

                        let snapshot_signature =
                            "Signature=54444af616c38775423dafed5497d19da7f5ef921d04bc89002c8f9e2dc8586c";
                        assert!(
                            auth_header.contains(snapshot_signature),
                            "authorization header signature did not match expected signature: got {}, expected it to contain {}",
                            auth_header,
                            snapshot_signature
                        );
                        """,
                        *RuntimeType.preludeScope,
                        "UserAgentInterceptor" to
                            AwsRuntimeType.awsRuntime(rc)
                                .resolve("user_agent::UserAgentInterceptor"),
                        "capture_request" to RuntimeType.captureRequest(rc),
                        "disable_interceptor" to
                            RuntimeType.smithyRuntimeApiClient(rc)
                                .resolve("client::interceptors::disable_interceptor"),
                    )
                }
            }
        }
    }
}
