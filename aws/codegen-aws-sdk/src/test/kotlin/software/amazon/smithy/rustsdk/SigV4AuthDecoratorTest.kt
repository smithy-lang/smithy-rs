/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rustsdk

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
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
            @required
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
        """.asSmithyModel(smithyVersion = "2.0")

    @Test
    fun unsignedPayloadSetsCorrectHeader() {
        awsSdkIntegrationTest(modelWithSigV4AuthScheme) { _, _ -> }
    }

    private val modelWithSigV4aAuthScheme =
        """
        namespace test

        use aws.auth#sigv4
        use aws.auth#sigv4a
        use aws.api#service
        use aws.protocols#restJson1
        use smithy.rules#endpointRuleSet
        use aws.auth#unsignedPayload
        use smithy.test#httpRequestTests

        @auth([sigv4a,sigv4])
        @sigv4(name: "dontcare")
        @sigv4a(name: "dontcare")
        @restJson1
        @endpointRuleSet({
            "version": "1.0",
            "rules": [
                {
                    "type": "endpoint",
                    "conditions": [],
                    "endpoint": {
                        "url": "https://example.com",
                        "properties": {
                            "authSchemes": [
                                {
                                    "name": "sigv4a",
                                    "signingRegionSet": ["*"],
                                    "signingName": "dontcare"
                                }
                            ]
                        }
                    }
                }
            ],
            "parameters": {
                "endpoint": { "required": true, "type": "string", "builtIn": "SDK::Endpoint" },
            }
        })
        @service(sdkId: "dontcare")
        service TestService { version: "2023-01-01", operations: [SomeOperation] }

        @streaming
        blob Bytestream

        structure SomeInput {
            @httpPayload
            @required
            something: Bytestream
         }

        structure SomeOutput { something: String }

        @http(uri: "/", method: "POST")
        operation SomeOperation { input: SomeInput, output: SomeOutput }
        """.asSmithyModel(smithyVersion = "2.0")

    @Test
    fun unsignedPayloadSetsCorrectHeaderForSigV4a() {
        awsSdkIntegrationTest(modelWithSigV4aAuthScheme) { clientCodegenContext, rustCrate ->
            val moduleUseName = clientCodegenContext.moduleUseName()
            val rc = clientCodegenContext.runtimeConfig

            rustCrate.integrationTest("sigv4a") {
                Attribute.featureGate("test-util").render(this)
                tokioTest("test_sigv4a_signing") {
                    rustTemplate(
                        """
                        let http_client = #{StaticReplayClient}::new(vec![#{ReplayEvent}::new(
                            #{Request}::builder()
                                .header("authorization", "AWS4-ECDSA-P256-SHA256 Credential=ANOTREAL/20090213/dontcare/aws4_request, SignedHeaders=content-length;content-type;host;x-amz-date;x-amz-region-set;x-amz-user-agent, Signature=3045022100b95d1c054ff04b676d12f0c893348606844d67ccf595981f0ca4968fae2eddfd022073e66edc0ad1da05b08392fccefa3ad69f8ec9393461033412fa05c55b749e9d")
                                .uri("https://example.com")
                                .body(#{SdkBody}::from("Hello, world!"))
                                .unwrap(),
                            #{Response}::builder().status(200).body(#{SdkBody}::empty()).unwrap(),
                        )]);
                        let config = $moduleUseName::Config::builder()
                            .http_client(http_client.clone())
                            .endpoint_url("https://example.com")
                            .behavior_version_latest()
                            .with_test_defaults()
                            .build();
                        let client = $moduleUseName::Client::from_conf(config);
                        let _ = client.some_operation().something(#{ByteStream}::from_static(b"Hello, world!")).send().await;

                        http_client.assert_requests_match(&["authorization"]);
                        let auth_header = http_client.actual_requests().next().unwrap().headers().get(http::header::AUTHORIZATION).unwrap();
                        assert!(auth_header.contains("AWS4-ECDSA-P256-SHA256"));
                        """,
                        "ByteStream" to RuntimeType.byteStream(rc),
                        "Credentials" to AwsRuntimeType.awsCredentialTypesTestUtil(rc).resolve("Credentials"),
                        "Region" to AwsRuntimeType.awsTypes(rc).resolve("region::Region"),
                        "ReplayEvent" to CargoDependency.smithyHttpClientTestUtil(rc).toType().resolve("test_util::ReplayEvent"),
                        "Request" to RuntimeType.HttpRequest,
                        "Response" to RuntimeType.HttpResponse,
                        "SdkBody" to RuntimeType.sdkBody(rc),
                        "StaticReplayClient" to CargoDependency.smithyHttpClientTestUtil(rc).toType().resolve("test_util::StaticReplayClient"),
                        "tracing_subscriber" to RuntimeType.TracingSubscriber,
                    )
                }
            }
        }
    }
}
