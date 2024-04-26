/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest

class HttpRequestCompressionDecoratorTest {
    companion object {
        // Can't use the dollar sign in a multiline string with doing it like this.
        private const val PREFIX = "\$version: \"2\""
        val model =
            """
            $PREFIX
            namespace test

            use aws.api#service
            use aws.auth#sigv4
            use aws.protocols#restJson1
            use smithy.rules#endpointRuleSet

            @service(sdkId: "dontcare")
            @restJson1
            @sigv4(name: "dontcare")
            @auth([sigv4])
            @endpointRuleSet({
                "version": "1.0",
                "rules": [{ "type": "endpoint", "conditions": [], "endpoint": { "url": "https://example.com" } }],
                "parameters": {
                    "Region": { "required": false, "type": "String", "builtIn": "AWS::Region" },
                }
            })
            service TestService {
                version: "2023-01-01",
                operations: [SomeOperation]
            }

            @streaming
            blob StreamingBlob

            blob NonStreamingBlob

            @http(uri: "/SomeOperation", method: "POST")
            @optionalAuth
            @requestCompression(encodings: ["gzip"])
            operation SomeOperation {
                input: SomeInput,
                output: SomeOutput
            }

            @input
            structure SomeInput {
                @httpPayload
                @required
                body: NonStreamingBlob
            }

            @output
            structure SomeOutput {}

            @http(uri: "/SomeOperation", method: "POST")
            @optionalAuth
            @requestCompression(encodings: ["gzip"])
            operation SomeStreamingOperation {
                input: SomeStreamingInput,
                output: SomeStreamingOutput
            }

            @input
            structure SomeStreamingInput {
                @httpPayload
                @required
                body: StreamingBlob
            }

            @output
            structure SomeStreamingOutput {}

            """.asSmithyModel()
    }

    @Test
    fun smokeTestSdkCodegen() {
        awsSdkIntegrationTest(model) { _, _ ->
            // it should compile
        }
    }

    @Test
    fun requestCompressionWorks() {
        awsSdkIntegrationTest(model) { context, rustCrate ->
            val rc = context.runtimeConfig
            val moduleName = context.moduleUseName()
            rustCrate.integrationTest("request_compression") {
                rustTemplate(
                    """
                    ##![cfg(feature = "test-util")]

                    use #{ByteStream};
                    use #{Blob};
                    use #{Region};

                    const UNCOMPRESSED_INPUT: &[u8] = b"hello world";
                    const COMPRESSED_OUTPUT: &[u8] = &[
                        31, 139, 8, 0, 0, 0, 0, 0, 0, 255, 147, 239, 230, 96, 0, 131, 255, 167, 61, 206, 158, 60, 25, 174, 113, 94, 255, 148, 39, 35, 67, 171, 160, 23, 47, 55, 80, 20, 0, 99, 154, 216, 40, 31, 0, 0, 0
                    ];

                    ##[#{tokio}::test]
                    async fn request_compression() {
                        let (http_client, rx) = #{capture_request}(None);
                        let config = $moduleName::Config::builder()
                            .region(Region::from_static("doesntmatter"))
                            .with_test_defaults()
                            .http_client(http_client)
                            .build();
                        let client = $moduleName::Client::from_conf(config);
                        let _ = client.some_operation().body(Blob::new(UNCOMPRESSED_INPUT)).send().await;
                        let request = rx.expect_request();
                        // Check that the content-encoding header is set to "gzip"
                        assert_eq!("gzip", request.headers().get("content-encoding").unwrap());

                        let compressed_body = ByteStream::from(request.into_body()).collect().await.unwrap().to_vec();
                        // Assert input body was compressed
                        assert_eq!(COMPRESSED_OUTPUT, compressed_body.as_slice())
                    }
                    """,
                    *preludeScope,
                    "ByteStream" to RuntimeType.smithyTypes(rc).resolve("byte_stream::ByteStream"),
                    "Blob" to RuntimeType.smithyTypes(rc).resolve("Blob"),
                    "Region" to AwsRuntimeType.awsTypes(rc).resolve("region::Region"),
                    "tokio" to CargoDependency.Tokio.toType(),
                    "capture_request" to RuntimeType.captureRequest(rc),
                )
            }
        }
    }
}
