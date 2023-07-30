/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.client.testutil.TestCodegenSettings
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest

// If any of these tests fail, and you want to understand why, run them with logging:
// ```
// ./gradlew codegen-client:test --tests software.amazon.smithy.rust.codegen.client.HttpVersionListGeneratorTest --info
// ```

// TODO(enableNewSmithyRuntimeCleanup): Delete this test (incomplete http version support wasn't ported to orchestrator)
internal class HttpVersionListGeneratorTest {
    @Test
    fun `http version list integration test (no preferred version)`() {
        val model = """
            namespace com.example

            use aws.protocols#awsJson1_0

            @awsJson1_0
            @aws.api#service(sdkId: "Test", endpointPrefix: "differentPrefix")
            service TestService {
                operations: [SayHello],
                version: "1"
            }

            @endpoint(hostPrefix: "test123.{greeting}.")
            operation SayHello {
                input: SayHelloInput
            }

            structure SayHelloInput {
                @required
                @hostLabel
                greeting: String
            }
        """.asSmithyModel()
        clientIntegrationTest(model, TestCodegenSettings.middlewareModeTestParams) { clientCodegenContext, rustCrate ->
            val moduleName = clientCodegenContext.moduleUseName()
            rustCrate.integrationTest("http_version_list") {
                Attribute.TokioTest.render(this)
                rust(
                    """
                    async fn test_http_version_list_defaults() {
                        let conf = $moduleName::Config::builder().build();
                        let op = $moduleName::operation::say_hello::SayHelloInput::builder()
                            .greeting("hello")
                            .build().expect("valid operation")
                            .make_operation(&conf).await.expect("hello is a valid prefix");
                        let properties = op.properties();
                        let actual_http_versions = properties.get::<Vec<http::Version>>()
                            .expect("http versions list should be in property bag");
                        let expected_http_versions = &vec![http::Version::HTTP_11];
                        assert_eq!(actual_http_versions, expected_http_versions);
                    }
                    """,
                )
            }
        }
    }

    @Test
    fun `http version list integration test (http)`() {
        val model = """
            namespace com.example

            use aws.protocols#restJson1

            @restJson1(http: ["http/1.1", "h2"])
            @aws.api#service(sdkId: "Test", endpointPrefix: "differentPrefix")
            service TestService {
                operations: [SayHello],
                version: "1"
            }

            @http(method: "PUT", uri: "/input", code: 200)
            @idempotent
            @endpoint(hostPrefix: "test123.{greeting}.")
            operation SayHello {
                input: SayHelloInput
            }

            structure SayHelloInput {
                @required
                @hostLabel
                greeting: String
            }
        """.asSmithyModel()
        clientIntegrationTest(model, TestCodegenSettings.middlewareModeTestParams) { clientCodegenContext, rustCrate ->
            val moduleName = clientCodegenContext.moduleUseName()
            rustCrate.integrationTest("validate_http") {
                Attribute.TokioTest.render(this)
                rust(
                    """
                    async fn test_http_version_list_defaults() {
                        let conf = $moduleName::Config::builder().build();
                        let op = $moduleName::operation::say_hello::SayHelloInput::builder()
                            .greeting("hello")
                            .build().expect("valid operation")
                            .make_operation(&conf).await.expect("hello is a valid prefix");
                        let properties = op.properties();
                        let actual_http_versions = properties.get::<Vec<http::Version>>()
                            .expect("http versions list should be in property bag");
                        let expected_http_versions = &vec![http::Version::HTTP_11, http::Version::HTTP_2];
                        assert_eq!(actual_http_versions, expected_http_versions);
                    }
                    """,
                )
            }
        }
    }

    @Test
    fun `http version list integration test (eventStreamHttp)`() {
        val model = """
            namespace com.example

            use aws.protocols#restJson1

            @restJson1(http: ["h2", "http/1.1"], eventStreamHttp: ["h2"])
            @aws.api#service(sdkId: "Test")
            service TestService {
                operations: [SayHello],
                version: "1"
            }

            @idempotent
            @http(uri: "/test", method: "PUT")
            operation SayHello {
                input: SayHelloInput,
                output: SayHelloOutput
            }

            structure SayHelloInput {}

            structure SayHelloOutput {
                @httpPayload
                greeting: SomeStream
            }

            structure Something { stuff: SomethingElse }

            structure SomethingElse {
                version: String
            }

            @streaming
            union SomeStream {
                Something: Something,
            }
        """.asSmithyModel()

        clientIntegrationTest(
            model,
            TestCodegenSettings.middlewareModeTestParams.copy(addModuleToEventStreamAllowList = true),
        ) { clientCodegenContext, rustCrate ->
            val moduleName = clientCodegenContext.moduleUseName()
            rustCrate.integrationTest("validate_eventstream_http") {
                Attribute.TokioTest.render(this)
                rust(
                    """
                    async fn test_http_version_list_defaults() {
                        let conf = $moduleName::Config::builder().build();
                        let op = $moduleName::operation::say_hello::SayHelloInput::builder()
                            .build().expect("valid operation")
                            .make_operation(&conf).await.unwrap();
                        let properties = op.properties();
                        let actual_http_versions = properties.get::<Vec<http::Version>>()
                            .expect("http versions list should be in property bag");
                        let expected_http_versions = &vec![http::Version::HTTP_2];
                        assert_eq!(actual_http_versions, expected_http_versions);
                    }
                    """,
                )
            }
        }
    }
}
