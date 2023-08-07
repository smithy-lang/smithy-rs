/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.ValueSource
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.traits.EndpointTrait
import software.amazon.smithy.rust.codegen.client.smithy.SmithyRuntimeMode
import software.amazon.smithy.rust.codegen.client.testutil.TestCodegenSettings
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.client.testutil.testSymbolProvider
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.implBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.generators.operationBuildError
import software.amazon.smithy.rust.codegen.core.testutil.TestRuntimeConfig
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.core.util.lookup
import kotlin.io.path.ExperimentalPathApi

internal class EndpointTraitBindingsTest {
    @Test
    fun `generate correct format strings`() {
        val epTrait = EndpointTrait.builder().hostPrefix("{foo}.data").build()
        epTrait.prefixFormatString() shouldBe ("\"{foo}.data\"")
    }

    @ParameterizedTest
    @ValueSource(strings = ["middleware", "orchestrator"])
    fun `generate endpoint prefixes`(smithyRuntimeModeStr: String) {
        val smithyRuntimeMode = SmithyRuntimeMode.fromString(smithyRuntimeModeStr)
        val model = """
            namespace test
            @readonly
            @endpoint(hostPrefix: "{foo}a.data.")
            operation GetStatus {
                input: GetStatusInput,
            }
            structure GetStatusInput {
                @required
                @hostLabel
                foo: String
            }
        """.asSmithyModel()
        val operationShape: OperationShape = model.lookup("test#GetStatus")
        val symbolProvider = testSymbolProvider(model)
        val endpointBindingGenerator = EndpointTraitBindings(
            model,
            symbolProvider,
            TestRuntimeConfig,
            operationShape,
            operationShape.expectTrait(EndpointTrait::class.java),
        )
        val project = TestWorkspace.testProject()
        project.withModule(RustModule.private("test")) {
            rust(
                """
                struct GetStatusInput {
                    foo: Option<String>
                }
                """,
            )
            implBlock(symbolProvider.toSymbol(model.lookup("test#GetStatusInput"))) {
                rustBlock(
                    "fn endpoint_prefix(&self) -> std::result::Result<#T::endpoint::EndpointPrefix, #T>",
                    RuntimeType.smithyHttp(TestRuntimeConfig),
                    TestRuntimeConfig.operationBuildError(),
                ) {
                    endpointBindingGenerator.render(this, "self", smithyRuntimeMode)
                }
            }
            unitTest(
                "valid_prefix",
                """
                let inp = GetStatusInput { foo: Some("test_value".to_string()) };
                let prefix = inp.endpoint_prefix().unwrap();
                assert_eq!(prefix.as_str(), "test_valuea.data.");
                """,
            )
            unitTest(
                "invalid_prefix",
                """
                // not a valid URI component
                let inp = GetStatusInput { foo: Some("test value".to_string()) };
                inp.endpoint_prefix().expect_err("invalid uri component");
                """,
            )

            unitTest(
                "unset_prefix",
                """
                // unset is invalid
                let inp = GetStatusInput { foo: None };
                inp.endpoint_prefix().expect_err("invalid uri component");
                """,
            )

            unitTest(
                "empty_prefix",
                """
                // empty is invalid
                let inp = GetStatusInput { foo: Some("".to_string()) };
                inp.endpoint_prefix().expect_err("empty label is invalid");
                """,
            )
        }

        project.compileAndTest()
    }

    // TODO(enableNewSmithyRuntimeCleanup): Delete this test (replaced by the @Test below it)
    @ExperimentalPathApi
    @Test
    fun `endpoint integration test middleware`() {
        val model = """
            namespace com.example
            use aws.protocols#awsJson1_0
            @awsJson1_0
            @aws.api#service(sdkId: "Test", endpointPrefix: "differentprefix")
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
            rustCrate.integrationTest("test_endpoint_prefix") {
                Attribute.TokioTest.render(this)
                rust(
                    """
                    async fn test_endpoint_prefix() {
                        let conf = $moduleName::Config::builder().build();
                        $moduleName::operation::say_hello::SayHelloInput::builder()
                            .greeting("hey there!").build().expect("input is valid")
                            .make_operation(&conf).await.expect_err("no spaces or exclamation points in ep prefixes");
                        let op = $moduleName::operation::say_hello::SayHelloInput::builder()
                            .greeting("hello")
                            .build().expect("valid operation")
                            .make_operation(&conf).await.expect("hello is a valid prefix");
                        let properties = op.properties();
                        let prefix = properties.get::<aws_smithy_http::endpoint::EndpointPrefix>()
                            .expect("prefix should be in config")
                            .as_str();
                        assert_eq!(prefix, "test123.hello.");
                    }
                    """,
                )
            }
        }
    }

    @ExperimentalPathApi
    @Test
    fun `endpoint integration test`() {
        val model = """
            namespace com.example
            use aws.protocols#awsJson1_0
            use smithy.rules#endpointRuleSet

            @awsJson1_0
            @aws.api#service(sdkId: "Test", endpointPrefix: "differentprefix")
            @endpointRuleSet({
                "version": "1.0",
                "rules": [{
                    "conditions": [],
                    "type": "endpoint",
                    "endpoint": {
                        "url": "https://example.com",
                        "properties": {}
                    }
                }],
                "parameters": {}
            })
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
        clientIntegrationTest(model) { clientCodegenContext, rustCrate ->
            val moduleName = clientCodegenContext.moduleUseName()
            rustCrate.integrationTest("test_endpoint_prefix") {
                Attribute.TokioTest.render(this)
                rustTemplate(
                    """
                    async fn test_endpoint_prefix() {
                        use #{aws_smithy_client}::test_connection::capture_request;
                        use aws_smithy_http::body::SdkBody;
                        use aws_smithy_http::endpoint::EndpointPrefix;
                        use aws_smithy_runtime_api::box_error::BoxError;
                        use aws_smithy_runtime_api::client::runtime_components::RuntimeComponents;
                        use aws_smithy_types::config_bag::ConfigBag;
                        use std::sync::atomic::{AtomicU32, Ordering};
                        use std::sync::{Arc, Mutex};
                        use $moduleName::{
                            config::interceptors::BeforeTransmitInterceptorContextRef,
                            config::Interceptor,
                            error::DisplayErrorContext,
                            {Client, Config},
                        };

                        ##[derive(Clone, Debug, Default)]
                        struct TestInterceptor {
                            called: Arc<AtomicU32>,
                            last_endpoint_prefix: Arc<Mutex<Option<EndpointPrefix>>>,
                        }
                        impl Interceptor for TestInterceptor {
                            fn name(&self) -> &'static str {
                                "TestInterceptor"
                            }

                            fn read_before_transmit(
                                &self,
                                _context: &BeforeTransmitInterceptorContextRef<'_>,
                                _runtime_components: &RuntimeComponents,
                                cfg: &mut ConfigBag,
                            ) -> Result<(), BoxError> {
                                self.called.fetch_add(1, Ordering::Relaxed);
                                if let Some(prefix) = cfg.load::<EndpointPrefix>() {
                                    self.last_endpoint_prefix
                                        .lock()
                                        .unwrap()
                                        .replace(prefix.clone());
                                }
                                Ok(())
                            }
                        }

                        let (conn, _r) = capture_request(Some(
                            http::Response::builder()
                                .status(200)
                                .body(SdkBody::from(""))
                                .unwrap(),
                        ));
                        let interceptor = TestInterceptor::default();
                        let config = Config::builder()
                            .http_connector(conn)
                            .interceptor(interceptor.clone())
                            .build();
                        let client = Client::from_conf(config);
                        let err = dbg!(client.say_hello().greeting("hey there!").send().await)
                            .expect_err("the endpoint should be invalid since it has an exclamation mark in it");
                        let err_fmt = format!("{}", DisplayErrorContext(err));
                        assert!(
                            err_fmt.contains("endpoint prefix could not be built"),
                            "expected '{}' to contain 'endpoint prefix could not be built'",
                            err_fmt
                        );

                        assert!(
                            interceptor.called.load(Ordering::Relaxed) == 0,
                            "the interceptor should not have been called since endpoint resolution failed"
                        );

                        dbg!(client.say_hello().greeting("hello").send().await)
                            .expect("hello is a valid endpoint prefix");
                        assert!(
                            interceptor.called.load(Ordering::Relaxed) == 1,
                            "the interceptor should have been called"
                        );
                        assert_eq!(
                            "test123.hello.",
                            interceptor
                                .last_endpoint_prefix
                                .lock()
                                .unwrap()
                                .clone()
                                .unwrap()
                                .as_str()
                        );
                    }
                    """,
                    "aws_smithy_client" to CargoDependency.smithyClient(clientCodegenContext.runtimeConfig)
                        .toDevDependency().withFeature("test-util").toType(),
                )
            }
        }
    }
}
