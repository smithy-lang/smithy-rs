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
import software.amazon.smithy.rust.codegen.core.testutil.runWithWarnings

// TODO(enableNewSmithyRuntimeCleanup): Delete this test (replaced by HttpAuthDecoratorTest)
internal class ApiKeyAuthDecoratorTest {
    private val modelQuery = """
        namespace test

        use aws.api#service
        use aws.protocols#restJson1

        @service(sdkId: "Test Api Key Auth")
        @restJson1
        @httpApiKeyAuth(name: "api_key", in: "query")
        @auth([httpApiKeyAuth])
        service TestService {
            version: "2023-01-01",
            operations: [SomeOperation]
        }

        structure SomeOutput {
            someAttribute: Long,
            someVal: String
        }

        @http(uri: "/SomeOperation", method: "GET")
        operation SomeOperation {
            output: SomeOutput
        }
    """.asSmithyModel()

    @Test
    fun `set an api key in query parameter`() {
        val testDir = clientIntegrationTest(
            modelQuery,
            // just run integration tests
            TestCodegenSettings.middlewareModeTestParams
                .copy(command = { "cargo test --test *".runWithWarnings(it) }),
        ) { clientCodegenContext, rustCrate ->
            rustCrate.integrationTest("api_key_present_in_property_bag") {
                val moduleName = clientCodegenContext.moduleUseName()
                Attribute.TokioTest.render(this)
                rust(
                    """
                    async fn api_key_present_in_property_bag() {
                        use aws_smithy_http_auth::api_key::AuthApiKey;
                        let api_key_value = "some-api-key";
                        let conf = $moduleName::Config::builder()
                            .api_key(AuthApiKey::new(api_key_value))
                            .build();
                        let operation = $moduleName::operation::some_operation::SomeOperationInput::builder()
                            .build()
                            .expect("input is valid")
                            .make_operation(&conf)
                            .await
                            .expect("valid operation");
                        let props = operation.properties();
                        let api_key_config = props.get::<AuthApiKey>().expect("api key in the bag");
                        assert_eq!(
                            api_key_config,
                            &AuthApiKey::new(api_key_value),
                        );
                    }
                    """,
                )
            }

            rustCrate.integrationTest("api_key_auth_is_set_in_query") {
                val moduleName = clientCodegenContext.moduleUseName()
                Attribute.TokioTest.render(this)
                rust(
                    """
                    async fn api_key_auth_is_set_in_query() {
                        use aws_smithy_http_auth::api_key::AuthApiKey;
                        let api_key_value = "some-api-key";
                        let conf = $moduleName::Config::builder()
                            .api_key(AuthApiKey::new(api_key_value))
                            .build();
                        let operation = $moduleName::operation::some_operation::SomeOperationInput::builder()
                            .build()
                            .expect("input is valid")
                            .make_operation(&conf)
                            .await
                            .expect("valid operation");
                        assert_eq!(
                            operation.request().uri().query(),
                            Some("api_key=some-api-key"),
                        );
                    }
                    """,
                )
            }
        }
        "cargo clippy".runWithWarnings(testDir)
    }

    private val modelHeader = """
        namespace test

        use aws.api#service
        use aws.protocols#restJson1

        @service(sdkId: "Test Api Key Auth")
        @restJson1
        @httpApiKeyAuth(name: "authorization", in: "header", scheme: "ApiKey")
        @auth([httpApiKeyAuth])
        service TestService {
            version: "2023-01-01",
            operations: [SomeOperation]
        }

        structure SomeOutput {
            someAttribute: Long,
            someVal: String
        }

        @http(uri: "/SomeOperation", method: "GET")
        operation SomeOperation {
            output: SomeOutput
        }
    """.asSmithyModel()

    @Test
    fun `set an api key in http header`() {
        val testDir = clientIntegrationTest(
            modelHeader,
            // just run integration tests
            TestCodegenSettings.middlewareModeTestParams
                .copy(command = { "cargo test --test *".runWithWarnings(it) }),
        ) { clientCodegenContext, rustCrate ->
            rustCrate.integrationTest("api_key_auth_is_set_in_http_header") {
                val moduleName = clientCodegenContext.moduleUseName()
                Attribute.TokioTest.render(this)
                rust(
                    """
                    async fn api_key_auth_is_set_in_http_header() {
                        use aws_smithy_http_auth::api_key::AuthApiKey;
                        let api_key_value = "some-api-key";
                        let conf = $moduleName::Config::builder()
                            .api_key(AuthApiKey::new(api_key_value))
                            .build();
                        let operation = $moduleName::operation::some_operation::SomeOperationInput::builder()
                            .build()
                            .expect("input is valid")
                            .make_operation(&conf)
                            .await
                            .expect("valid operation");
                        let props = operation.properties();
                        let api_key_config = props.get::<AuthApiKey>().expect("api key in the bag");
                        assert_eq!(
                            api_key_config,
                            &AuthApiKey::new(api_key_value),
                        );
                        assert_eq!(
                            operation.request().headers().contains_key("authorization"),
                            true,
                        );
                    }
                    """,
                )
            }
        }
        "cargo clippy".runWithWarnings(testDir)
    }
}
