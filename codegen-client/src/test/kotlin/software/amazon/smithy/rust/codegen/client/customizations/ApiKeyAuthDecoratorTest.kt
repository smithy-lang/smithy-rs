/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.customizations

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.client.smithy.customizations.ApiKeyAuthDecorator
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.client.testutil.testCodegenContext
import software.amazon.smithy.rust.codegen.client.testutil.testSymbolProvider
import software.amazon.smithy.rust.codegen.client.testutil.validateConfigCustomizations
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest
import software.amazon.smithy.rust.codegen.core.testutil.runWithWarnings
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.core.testutil.TokioTest
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.unitTest


internal class ApiKeyAuthDecoratorTest {
    private val model = """
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
    fun `set an api key in the property bag`() {
        val testDir = clientIntegrationTest(
            model,
            additionalDecorators = listOf(ApiKeyAuthDecorator()),
            // just run integration tests
            command = { "cargo test --test *".runWithWarnings(it) },
        ) { clientCodegenContext, rustCrate ->
            rustCrate.integrationTest("api_key_config_test") {
                val moduleName = clientCodegenContext.moduleUseName()
                TokioTest.render(this)
                rust(
                    """
                    async fn api_key_is_set() {
                        use aws_smithy_types::auth::AuthApiKey;
                        let api_key_value = "some-api-key";
                        let conf = $moduleName::Config::builder()
                            .api_key(AuthApiKey::new(api_key_value))
                            .build();
                        let operation = $moduleName::operation::ListMappings::builder()
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
        }
        "cargo clippy".runWithWarnings(testDir)
    }
}
