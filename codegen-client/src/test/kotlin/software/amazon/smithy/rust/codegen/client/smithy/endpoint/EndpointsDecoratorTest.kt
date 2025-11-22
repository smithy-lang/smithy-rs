/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.endpoint

import io.kotest.assertions.throwables.shouldThrow
import io.kotest.matchers.string.shouldContain
import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest
import software.amazon.smithy.rust.codegen.core.testutil.runWithWarnings
import software.amazon.smithy.rust.codegen.core.util.CommandError
import software.amazon.smithy.rust.codegen.core.util.runCommand

/**
 * End-to-end test of endpoint resolvers, attaching a real resolver to a fully generated service
 */
class EndpointsDecoratorTest {
    val model =
        """
        namespace test

        use smithy.rules#endpointRuleSet
        use smithy.rules#endpointTests

        use smithy.rules#clientContextParams
        use smithy.rules#staticContextParams
        use smithy.rules#operationContextParams
        use smithy.rules#contextParam
        use aws.protocols#awsJson1_1

        @awsJson1_1
        @endpointRuleSet({
            "version": "1.0",
            "rules": [{
                "conditions": [
                    {"fn": "isSet", "argv": [{"ref":"Region"}]},
                    {"fn": "isSet", "argv": [{"ref":"ABoolParam"}]},
                    {"fn": "booleanEquals", "argv": [{"ref": "ABoolParam"}, false]}
                ],
                "type": "endpoint",
                "endpoint": {
                    "url": "https://www.{Region}.example.com",
                    "properties": {
                        "first-properties": {
                            "z-first": "zazz",
                            "y-second": "bar",
                            "x-third": "baz"
                        },
                        "second-properties": [1,2,3]
                    }
                }
            }],
            "parameters": {
                "Bucket": { "required": false, "type": "string" },
                "Region": { "required": false, "type": "string", "builtIn": "AWS::Region" },
                "BuiltInWithDefault": { "required": true, "type": "string", "builtIn": "AWS::DefaultBuiltIn", "default": "some-default" },
                "BoolBuiltInWithDefault": { "required": true, "type": "boolean", "builtIn": "AWS::FooBar", "default": true },
                "AStringParam": { "required": false, "type": "string" },
                "ABoolParam": { "required": false, "type": "boolean" },
                "AStringArrayParam": { "required": false, "type": "stringArray" },
                "JmesPathParamString": {"required": false, type: "string"},
                "JmesPathParamBoolean": {"required": false, type: "boolean"},
                "JmesPathParamStringArray": {"required": false, type: "stringArray"},
            }
        })
        @clientContextParams(
            AStringParam: {
                documentation: "string docs",
                type: "string"
            },
            ABoolParam: {
                documentation: "bool docs",
                type: "boolean"
            }
        )
        @endpointTests({
          "version": "1.0",
          "testCases": [
            {
              "documentation": "uriEncode when the string has nothing to encode returns the input",
              "params": {
                "Region": "test-region"
              },
              "operationInputs": [
                { "operationName": "TestOperation", "operationParams": { "nested": { "field": "test" } } }
              ],
              "expect": {
                "endpoint": {
                    "url": "https://failingtest.com"
                    "properties": {
                        "first-properties": {
                            "a-first": "zazz",
                            "b-second": "bar",
                            "c-third": "baz"
                        },
                        "second-properties": [1,2,3]
                    }
                }
              }
            }
         ]
        })
        service TestService {
            operations: [TestOperation]
        }

        @staticContextParams(
            Region: { value: "us-east-2" },
            AStringArrayParam: {value: ["a", "b", "c"]}
        )
        @operationContextParams(
            JmesPathParamString: {
                path: "nested.field",
            }
            JmesPathParamBoolean: {
                path: "nested.boolField",
            }
            JmesPathParamStringArray: {
                path: "keys(nested.mapField)",
            }
        )
        operation TestOperation {
            input: TestOperationInput
        }

        @input
        structure TestOperationInput {
            @contextParam(name: "Bucket")
            @required
            bucket: String,
            nested: NestedStructure
        }

        structure NestedStructure {
            field: String,
            boolField: Boolean,
            mapField: IntegerMap,
        }

        map IntegerMap {
            key: String,
            value: Integer
        }
        """.asSmithyModel(disableValidation = true)

    val bddModel =
        """
        ${"$"}version: "2.0"

        namespace test

        use aws.protocols#restJson1
        use smithy.rules#clientContextParams
        use smithy.rules#endpointBdd
        use smithy.rules#endpointRuleSet

        @clientContextParams(
            Region: {type: "string", documentation: "docs"}
            UseFips: {type: "boolean", documentation: "docs"}
        )
        @endpointBdd({
            version: "1.1"
            "parameters": {
                "Region": {
                    "required": true,
                    "documentation": "The AWS region",
                    "type": "string"
                },
                "UseFips": {
                    "required": true,
                    "default": false,
                    "documentation": "Use FIPS endpoints",
                    "type": "boolean"
                }
            },
            "conditions": [
                {
                    "fn": "booleanEquals",
                    "argv": [
                        {
                            "ref": "UseFips"
                        },
                        true
                    ]
                }
            ],
            "results": [
                {
                    "conditions": [],
                    "endpoint": {
                        "url": "https://service-fips.{Region}.amazonaws.com",
                        "properties": {},
                        "headers": {}
                    },
                    "type": "endpoint"
                },
                {
                    "conditions": [],
                    "endpoint": {
                        "url": "https://service.{Region}.amazonaws.com",
                        "properties": {},
                        "headers": {}
                    },
                    "type": "endpoint"
                }
            ],
            "root": 2,
            "nodeCount": 2,
            "nodes": "/////wAAAAH/////AAAAAAX14QEF9eEC"
        })
        @restJson1
        service ServiceWithEndpointBdd {
            version: "2022-01-01"
            operations:[
                Echo
            ]
        }

        @http(method: "PUT", uri: "/echo")
        operation Echo {
            input := {
                string: String
            }
            output := {
                string: String
            }
            errors: [
                MyErrorA
                MyErrorB
            ]
        }

        @error("client")
        @httpError(401)
        structure MyErrorA {
            @required
            message: String
        }

        @error("server")
        @httpError(501)
        structure MyErrorB {
            @required
            message: String
        }
        """.trimIndent().asSmithyModel()

    @Test
    fun `resolve endpoint BDD`() {
        val testDir =
            clientIntegrationTest(
                bddModel,
                // Just run integration tests.
                IntegrationTestParams(command = { "cargo test --all-features --test *".runCommand(it) }),
            ) { clientCodegenContext, rustCrate ->
                rustCrate.integrationTest("endpoint_params_test") {
                    val moduleName = clientCodegenContext.moduleUseName()
                    Attribute.TokioTest.render(this)
                    rustTemplate(
                        """
                        async fn endpoint_params_are_set() {
                            use #{NeverClient};
                            use #{TokioSleep};
                            use aws_smithy_runtime_api::box_error::BoxError;
                            use aws_smithy_runtime_api::client::endpoint::EndpointResolverParams;
                            use aws_smithy_runtime_api::client::runtime_components::RuntimeComponents;
                            use aws_smithy_types::config_bag::ConfigBag;
                            use aws_smithy_types::endpoint::Endpoint;
                            use aws_smithy_types::timeout::TimeoutConfig;
                            use std::sync::atomic::AtomicBool;
                            use std::sync::atomic::Ordering;
                            use std::sync::Arc;
                            use std::time::Duration;
                            use $moduleName::{
                                config::endpoint::Params, config::interceptors::BeforeTransmitInterceptorContextRef,
                                config::Intercept, config::SharedAsyncSleep, types::NestedStructure, Client, Config,
                            };

                            ##[derive(Clone, Debug, Default)]
                            struct TestInterceptor {
                                called: Arc<AtomicBool>,
                            }
                            impl Intercept for TestInterceptor {
                                fn name(&self) -> &'static str {
                                    "TestInterceptor"
                                }

                                fn read_before_transmit(
                                    &self,
                                    _context: &BeforeTransmitInterceptorContextRef<'_>,
                                    _runtime_components: &RuntimeComponents,
                                    cfg: &mut ConfigBag,
                                ) -> Result<(), BoxError> {
                                    let params = cfg
                                        .load::<EndpointResolverParams>()
                                        .expect("params set in config");
                                    let preset_params: &Params = params.get().expect("correct type");
                                    let manual_params: &Params = &Params::builder()
                                        .bucket("bucket-name".to_string())
                                        .built_in_with_default("some-default")
                                        .bool_built_in_with_default(true)
                                        .a_bool_param(false)
                                        .a_string_param("hello".to_string())
                                        .region("us-east-2".to_string())
                                        .a_string_array_param(
                                            vec!["a", "b", "c"]
                                                .iter()
                                                .map(ToString::to_string)
                                                .collect::<Vec<_>>(),
                                        )
                                        .jmes_path_param_string_array(vec!["key2".to_string(), "key1".to_string()])
                                        .jmes_path_param_string("nested-field")
                                        .build()
                                        .unwrap();

                                    // The params struct for this test contains a vec sourced from the JMESPath keys function which
                                    // does not guarantee the order. Due to this we cannot compare the preset_params with the
                                    // manual_params directly, instead we must assert equlaity field by field.
                                    assert_eq!(preset_params.bucket(), manual_params.bucket());
                                    assert_eq!(preset_params.region(), manual_params.region());
                                    assert_eq!(
                                        preset_params.a_string_param(),
                                        manual_params.a_string_param()
                                    );
                                    assert_eq!(
                                        preset_params.built_in_with_default(),
                                        manual_params.built_in_with_default()
                                    );
                                    assert_eq!(
                                        preset_params.bool_built_in_with_default(),
                                        manual_params.bool_built_in_with_default()
                                    );
                                    assert_eq!(preset_params.a_bool_param(), manual_params.a_bool_param());
                                    assert_eq!(
                                        preset_params.a_string_array_param(),
                                        manual_params.a_string_array_param()
                                    );
                                    assert_eq!(
                                        preset_params.jmes_path_param_string(),
                                        manual_params.jmes_path_param_string()
                                    );
                                    assert_eq!(
                                        preset_params.jmes_path_param_boolean(),
                                        manual_params.jmes_path_param_boolean()
                                    );
                                    assert!(preset_params
                                        .jmes_path_param_string_array()
                                        .unwrap()
                                        .contains(&"key1".to_string()));
                                    assert!(preset_params
                                        .jmes_path_param_string_array()
                                        .unwrap()
                                        .contains(&"key2".to_string()));

                                    let endpoint = cfg.load::<Endpoint>().expect("endpoint set in config");
                                    assert_eq!(endpoint.url(), "https://www.us-east-2.example.com");

                                    self.called.store(true, Ordering::Relaxed);
                                    Ok(())
                                }
                            }

                            let interceptor = TestInterceptor::default();
                            let config = Config::builder()
                                .behavior_version_latest()
                                .http_client(NeverClient::new())
                                .interceptor(interceptor.clone())
                                .timeout_config(
                                    TimeoutConfig::builder()
                                        .operation_timeout(Duration::from_millis(30))
                                        .build(),
                                )
                                .sleep_impl(SharedAsyncSleep::new(TokioSleep::new()))
                                .a_string_param("hello")
                                .a_bool_param(false)
                                .build();
                            let client = Client::from_conf(config);

                            let _ = dbg!(
                                client
                                .test_operation()
                                .bucket("bucket-name")
                                .nested(
                                    NestedStructure::builder()
                                        .field("nested-field")
                                        .map_field("key1", 1)
                                        .map_field("key2", 2)
                                        .build()
                                )
                                .send()
                                .await
                            );
                            assert!(
                                interceptor.called.load(Ordering::Relaxed),
                                "the interceptor should have been called"
                            );

                            // bucket_name is unset and marked as required on the model, so we'll refuse to construct this request
                            let err = client.test_operation().send().await.expect_err("param missing");
                            assert_eq!(format!("{}", err), "failed to construct request");
                        }
                        """,
                        "NeverClient" to
                            CargoDependency.smithyHttpClientTestUtil(clientCodegenContext.runtimeConfig)
                                .toType().resolve("test_util::NeverClient"),
                        "TokioSleep" to
                            CargoDependency.smithyAsync(clientCodegenContext.runtimeConfig)
                                .withFeature("rt-tokio").toType().resolve("rt::sleep::TokioSleep"),
                    )
                }
            }
        // the model has an intentionally failing test—ensure it fails
        val failure = shouldThrow<CommandError> { "cargo test".runWithWarnings(testDir) }
        failure.output shouldContain "endpoint::test::test_1"
        failure.output shouldContain "https://failingtest.com"
        "cargo clippy".runWithWarnings(testDir)
    }

    @Test
    fun `resolve endpoint`() {
        val testDir =
            clientIntegrationTest(
                model,
                // Just run integration tests.
                IntegrationTestParams(command = { "cargo test --all-features --test *".runCommand(it) }),
            ) { clientCodegenContext, rustCrate ->
                rustCrate.integrationTest("endpoint_params_test") {
                    val moduleName = clientCodegenContext.moduleUseName()
                    Attribute.TokioTest.render(this)
                    rustTemplate(
                        """
                        async fn endpoint_params_are_set() {
                            use #{NeverClient};
                            use #{TokioSleep};
                            use aws_smithy_runtime_api::box_error::BoxError;
                            use aws_smithy_runtime_api::client::endpoint::EndpointResolverParams;
                            use aws_smithy_runtime_api::client::runtime_components::RuntimeComponents;
                            use aws_smithy_types::config_bag::ConfigBag;
                            use aws_smithy_types::endpoint::Endpoint;
                            use aws_smithy_types::timeout::TimeoutConfig;
                            use std::sync::atomic::AtomicBool;
                            use std::sync::atomic::Ordering;
                            use std::sync::Arc;
                            use std::time::Duration;
                            use $moduleName::{
                                config::endpoint::Params, config::interceptors::BeforeTransmitInterceptorContextRef,
                                config::Intercept, config::SharedAsyncSleep, types::NestedStructure, Client, Config,
                            };

                            ##[derive(Clone, Debug, Default)]
                            struct TestInterceptor {
                                called: Arc<AtomicBool>,
                            }
                            impl Intercept for TestInterceptor {
                                fn name(&self) -> &'static str {
                                    "TestInterceptor"
                                }

                                fn read_before_transmit(
                                    &self,
                                    _context: &BeforeTransmitInterceptorContextRef<'_>,
                                    _runtime_components: &RuntimeComponents,
                                    cfg: &mut ConfigBag,
                                ) -> Result<(), BoxError> {
                                    let params = cfg
                                        .load::<EndpointResolverParams>()
                                        .expect("params set in config");
                                    let preset_params: &Params = params.get().expect("correct type");
                                    let manual_params: &Params = &Params::builder()
                                        .bucket("bucket-name".to_string())
                                        .built_in_with_default("some-default")
                                        .bool_built_in_with_default(true)
                                        .a_bool_param(false)
                                        .a_string_param("hello".to_string())
                                        .region("us-east-2".to_string())
                                        .a_string_array_param(
                                            vec!["a", "b", "c"]
                                                .iter()
                                                .map(ToString::to_string)
                                                .collect::<Vec<_>>(),
                                        )
                                        .jmes_path_param_string_array(vec!["key2".to_string(), "key1".to_string()])
                                        .jmes_path_param_string("nested-field")
                                        .build()
                                        .unwrap();

                                    // The params struct for this test contains a vec sourced from the JMESPath keys function which
                                    // does not guarantee the order. Due to this we cannot compare the preset_params with the
                                    // manual_params directly, instead we must assert equlaity field by field.
                                    assert_eq!(preset_params.bucket(), manual_params.bucket());
                                    assert_eq!(preset_params.region(), manual_params.region());
                                    assert_eq!(
                                        preset_params.a_string_param(),
                                        manual_params.a_string_param()
                                    );
                                    assert_eq!(
                                        preset_params.built_in_with_default(),
                                        manual_params.built_in_with_default()
                                    );
                                    assert_eq!(
                                        preset_params.bool_built_in_with_default(),
                                        manual_params.bool_built_in_with_default()
                                    );
                                    assert_eq!(preset_params.a_bool_param(), manual_params.a_bool_param());
                                    assert_eq!(
                                        preset_params.a_string_array_param(),
                                        manual_params.a_string_array_param()
                                    );
                                    assert_eq!(
                                        preset_params.jmes_path_param_string(),
                                        manual_params.jmes_path_param_string()
                                    );
                                    assert_eq!(
                                        preset_params.jmes_path_param_boolean(),
                                        manual_params.jmes_path_param_boolean()
                                    );
                                    assert!(preset_params
                                        .jmes_path_param_string_array()
                                        .unwrap()
                                        .contains(&"key1".to_string()));
                                    assert!(preset_params
                                        .jmes_path_param_string_array()
                                        .unwrap()
                                        .contains(&"key2".to_string()));

                                    let endpoint = cfg.load::<Endpoint>().expect("endpoint set in config");
                                    assert_eq!(endpoint.url(), "https://www.us-east-2.example.com");

                                    self.called.store(true, Ordering::Relaxed);
                                    Ok(())
                                }
                            }

                            let interceptor = TestInterceptor::default();
                            let config = Config::builder()
                                .behavior_version_latest()
                                .http_client(NeverClient::new())
                                .interceptor(interceptor.clone())
                                .timeout_config(
                                    TimeoutConfig::builder()
                                        .operation_timeout(Duration::from_millis(30))
                                        .build(),
                                )
                                .sleep_impl(SharedAsyncSleep::new(TokioSleep::new()))
                                .a_string_param("hello")
                                .a_bool_param(false)
                                .build();
                            let client = Client::from_conf(config);

                            let _ = dbg!(
                                client
                                .test_operation()
                                .bucket("bucket-name")
                                .nested(
                                    NestedStructure::builder()
                                        .field("nested-field")
                                        .map_field("key1", 1)
                                        .map_field("key2", 2)
                                        .build()
                                )
                                .send()
                                .await
                            );
                            assert!(
                                interceptor.called.load(Ordering::Relaxed),
                                "the interceptor should have been called"
                            );

                            // bucket_name is unset and marked as required on the model, so we'll refuse to construct this request
                            let err = client.test_operation().send().await.expect_err("param missing");
                            assert_eq!(format!("{}", err), "failed to construct request");
                        }
                        """,
                        "NeverClient" to
                            CargoDependency.smithyHttpClientTestUtil(clientCodegenContext.runtimeConfig)
                                .toType().resolve("test_util::NeverClient"),
                        "TokioSleep" to
                            CargoDependency.smithyAsync(clientCodegenContext.runtimeConfig)
                                .withFeature("rt-tokio").toType().resolve("rt::sleep::TokioSleep"),
                    )
                }
            }
        // the model has an intentionally failing test—ensure it fails
        val failure = shouldThrow<CommandError> { "cargo test".runWithWarnings(testDir) }
        failure.output shouldContain "endpoint::test::test_1"
        failure.output shouldContain "https://failingtest.com"
        "cargo clippy".runWithWarnings(testDir)
    }
}
