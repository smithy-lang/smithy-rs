/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest

class TelemetryInputCaptureDecoratorTest {
    private fun codegenScope(runtimeConfig: RuntimeConfig): Array<Pair<String, Any>> =
        arrayOf(
            "capture_request" to RuntimeType.captureRequest(runtimeConfig),
            "SdkBody" to RuntimeType.sdkBody(runtimeConfig),
            "http_1x" to CargoDependency.Http1x.toType(),
            "CapturedTelemetryAttributes" to
                RuntimeType.smithyTypes(runtimeConfig).resolve("telemetry::CapturedTelemetryAttributes"),
            "Intercept" to RuntimeType.intercept(runtimeConfig),
            "FinalizerInterceptorContextRef" to
                RuntimeType.smithyRuntimeApiClient(runtimeConfig)
                    .resolve("client::interceptors::context::FinalizerInterceptorContextRef"),
            "RuntimeComponents" to
                RuntimeType.smithyRuntimeApiClient(runtimeConfig)
                    .resolve("client::runtime_components::RuntimeComponents"),
            "ConfigBag" to RuntimeType.configBag(runtimeConfig),
            "BoxError" to RuntimeType.boxError(runtimeConfig),
            "Mutex" to RuntimeType.std.resolve("sync::Mutex"),
            "Arc" to RuntimeType.std.resolve("sync::Arc"),
        )

    private val model =
        """
        ${'$'}version: "2.0"
        namespace com.example
        use aws.protocols#awsJson1_0
        @awsJson1_0
        service HelloService {
            operations: [SayHello],
            version: "1"
        }
        @optionalAuth
        operation SayHello { input: TestInput, output: TestOutput }

        structure TestInput {
           bucket: String,
           secret: SecretString,
           @required
           requiredName: String,
           kind: Kind,
        }

        @sensitive
        string SecretString

        enum Kind {
            FAST
            SLOW
        }

        structure TestOutput { }
        """.asSmithyModel(smithyVersion = "2")

    @Test
    fun `named input member is captured into the config bag`() {
        clientIntegrationTest(model) { codegenContext, rustCrate ->
            rustCrate.integrationTest("telemetry_input_capture") {
                val moduleName = codegenContext.moduleUseName()
                rustTemplate(
                    """
                    // An interceptor that reads what the capture interceptor wrote, at a later hook,
                    // so the test can assert the value survived onto the config bag.
                    ##[derive(Debug, Clone, Default)]
                    struct ObserveCaptured {
                        bucket: #{Arc}<#{Mutex}<Option<String>>>,
                        // Members that must never be captured even when named: a @sensitive target
                        // shape, and an enum (excluded by design).
                        secret: #{Arc}<#{Mutex}<Option<String>>>,
                        kind: #{Arc}<#{Mutex}<Option<String>>>,
                    }
                    impl #{Intercept} for ObserveCaptured {
                        fn name(&self) -> &'static str { "ObserveCaptured" }
                        fn read_after_execution(
                            &self,
                            _ctx: &#{FinalizerInterceptorContextRef}<'_>,
                            _rc: &#{RuntimeComponents},
                            cfg: &mut #{ConfigBag},
                        ) -> Result<(), #{BoxError}> {
                            let captured = cfg.load::<#{CapturedTelemetryAttributes}>();
                            let get = |name: &str| captured.and_then(|c| c.get(name).map(|v| v.to_string()));
                            *self.bucket.lock().unwrap() = get("bucket");
                            *self.secret.lock().unwrap() = get("secret");
                            *self.kind.lock().unwrap() = get("kind");
                            Ok(())
                        }
                    }
                    """,
                    *codegenScope(codegenContext.runtimeConfig),
                )

                Attribute.TokioTest.render(this)
                rustTemplate(
                    """
                    async fn named_member_is_captured() {
                        let (http_client, _r) = #{capture_request}(Some(
                            #{http_1x}::Response::builder()
                                .status(200)
                                .body(#{SdkBody}::from("{}"))
                                .unwrap(),
                        ));

                        let observer = ObserveCaptured::default();
                        let config = $moduleName::Config::builder()
                            .endpoint_url("http://localhost:1234")
                            .http_client(http_client.clone())
                            // Opt in: name every member. Only `bucket` is eligible; `secret` has a
                            // @sensitive target shape and `kind` is an enum, so neither may be
                            // captured even though they are named here.
                            .emit_input_attributes(["bucket", "secret", "kind"])
                            .interceptor(observer.clone())
                            .build();
                        let client = $moduleName::Client::from_conf(config);
                        let _ = client.say_hello()
                            .bucket("example-bucket")
                            .secret("do-not-capture")
                            .kind($moduleName::types::Kind::Fast)
                            .send()
                            .await
                            .expect("success");

                        let bucket = observer.bucket.lock().unwrap().clone();
                        assert_eq!(bucket.as_deref(), Some("example-bucket"), "bucket should be captured");

                        assert_eq!(*observer.secret.lock().unwrap(), None, "@sensitive member must not be captured");
                        assert_eq!(*observer.kind.lock().unwrap(), None, "enum member must not be captured");
                    }
                    """,
                    *codegenScope(codegenContext.runtimeConfig),
                )
            }
        }
    }

    @Test
    fun `capture-only members are captured but the two policies compose`() {
        clientIntegrationTest(model) { codegenContext, rustCrate ->
            rustCrate.integrationTest("telemetry_input_capture_only") {
                val moduleName = codegenContext.moduleUseName()
                rustTemplate(
                    """
                    ##[derive(Debug, Clone, Default)]
                    struct ObserveCaptured {
                        bucket: #{Arc}<#{Mutex}<Option<String>>>,
                        required_name: #{Arc}<#{Mutex}<Option<String>>>,
                    }
                    impl #{Intercept} for ObserveCaptured {
                        fn name(&self) -> &'static str { "ObserveCaptured" }
                        fn read_after_execution(
                            &self,
                            _ctx: &#{FinalizerInterceptorContextRef}<'_>,
                            _rc: &#{RuntimeComponents},
                            cfg: &mut #{ConfigBag},
                        ) -> Result<(), #{BoxError}> {
                            let captured = cfg.load::<#{CapturedTelemetryAttributes}>();
                            let get = |name: &str| captured.and_then(|c| c.get(name).map(|v| v.to_string()));
                            *self.bucket.lock().unwrap() = get("bucket");
                            *self.required_name.lock().unwrap() = get("requiredName");
                            Ok(())
                        }
                    }
                    """,
                    *codegenScope(codegenContext.runtimeConfig),
                )

                Attribute.TokioTest.render(this)
                rustTemplate(
                    """
                    async fn emit_and_capture_only_compose() {
                        let (http_client, _r) = #{capture_request}(Some(
                            #{http_1x}::Response::builder()
                                .status(200)
                                .body(#{SdkBody}::from("{}"))
                                .unwrap(),
                        ));

                        let observer = ObserveCaptured::default();
                        // `bucket` is emitted; `requiredName` is capture-only. Both should be
                        // captured into the bag (the interceptor captures the union of both sets).
                        let config = $moduleName::Config::builder()
                            .endpoint_url("http://localhost:1234")
                            .http_client(http_client.clone())
                            .emit_input_attributes(["bucket"])
                            .capture_input_attributes(["requiredName"])
                            .interceptor(observer.clone())
                            .build();
                        let client = $moduleName::Client::from_conf(config);
                        let _ = client.say_hello()
                            .bucket("example-bucket")
                            .required_name("job-42")
                            .send()
                            .await
                            .expect("success");

                        assert_eq!(
                            observer.bucket.lock().unwrap().as_deref(),
                            Some("example-bucket"),
                            "emitted member should be captured",
                        );
                        assert_eq!(
                            observer.required_name.lock().unwrap().as_deref(),
                            Some("job-42"),
                            "capture-only member should still be captured into the bag",
                        );
                    }
                    """,
                    *codegenScope(codegenContext.runtimeConfig),
                )
            }
        }
    }

    @Test
    fun `nothing is captured when the customer opts out`() {
        clientIntegrationTest(model) { codegenContext, rustCrate ->
            rustCrate.integrationTest("telemetry_input_capture_off") {
                val moduleName = codegenContext.moduleUseName()
                rustTemplate(
                    """
                    ##[derive(Debug, Clone, Default)]
                    struct ObserveCaptured {
                        present: #{Arc}<#{Mutex}<bool>>,
                    }
                    impl #{Intercept} for ObserveCaptured {
                        fn name(&self) -> &'static str { "ObserveCaptured" }
                        fn read_after_execution(
                            &self,
                            _ctx: &#{FinalizerInterceptorContextRef}<'_>,
                            _rc: &#{RuntimeComponents},
                            cfg: &mut #{ConfigBag},
                        ) -> Result<(), #{BoxError}> {
                            *self.present.lock().unwrap() =
                                cfg.load::<#{CapturedTelemetryAttributes}>().is_some();
                            Ok(())
                        }
                    }
                    """,
                    *codegenScope(codegenContext.runtimeConfig),
                )

                Attribute.TokioTest.render(this)
                rustTemplate(
                    """
                    async fn nothing_captured_by_default() {
                        let (http_client, _r) = #{capture_request}(Some(
                            #{http_1x}::Response::builder()
                                .status(200)
                                .body(#{SdkBody}::from("{}"))
                                .unwrap(),
                        ));

                        let observer = ObserveCaptured::default();
                        // No emit_input_attributes call: capture is off.
                        let config = $moduleName::Config::builder()
                            .endpoint_url("http://localhost:1234")
                            .http_client(http_client.clone())
                            .interceptor(observer.clone())
                            .build();
                        let client = $moduleName::Client::from_conf(config);
                        let _ = client.say_hello()
                            .bucket("example-bucket")
                            .send()
                            .await
                            .expect("success");

                        assert!(!*observer.present.lock().unwrap(), "nothing should be captured when off");
                    }
                    """,
                    *codegenScope(codegenContext.runtimeConfig),
                )
            }
        }
    }

    // Input structure carries @sensitive on the shape itself, which marks the whole shape
    // sensitive — no member may be captured even when named.
    private val sensitiveInputModel =
        """
        ${'$'}version: "2.0"
        namespace com.example
        use aws.protocols#awsJson1_0
        @awsJson1_0
        service HelloService {
            operations: [SayHello],
            version: "1"
        }
        @optionalAuth
        operation SayHello { input: TestInput, output: TestOutput }

        @sensitive
        structure TestInput {
           bucket: String,
        }

        structure TestOutput { }
        """.asSmithyModel(smithyVersion = "2")

    @Test
    fun `nothing is captured when the input shape itself is sensitive`() {
        clientIntegrationTest(sensitiveInputModel) { codegenContext, rustCrate ->
            rustCrate.integrationTest("telemetry_input_capture_sensitive_shape") {
                val moduleName = codegenContext.moduleUseName()
                rustTemplate(
                    """
                    ##[derive(Debug, Clone, Default)]
                    struct ObserveCaptured {
                        present: #{Arc}<#{Mutex}<bool>>,
                    }
                    impl #{Intercept} for ObserveCaptured {
                        fn name(&self) -> &'static str { "ObserveCaptured" }
                        fn read_after_execution(
                            &self,
                            _ctx: &#{FinalizerInterceptorContextRef}<'_>,
                            _rc: &#{RuntimeComponents},
                            cfg: &mut #{ConfigBag},
                        ) -> Result<(), #{BoxError}> {
                            *self.present.lock().unwrap() =
                                cfg.load::<#{CapturedTelemetryAttributes}>().is_some();
                            Ok(())
                        }
                    }
                    """,
                    *codegenScope(codegenContext.runtimeConfig),
                )

                Attribute.TokioTest.render(this)
                rustTemplate(
                    """
                    async fn sensitive_input_shape_captures_nothing() {
                        let (http_client, _r) = #{capture_request}(Some(
                            #{http_1x}::Response::builder()
                                .status(200)
                                .body(#{SdkBody}::from("{}"))
                                .unwrap(),
                        ));

                        let observer = ObserveCaptured::default();
                        // Name the member anyway; the @sensitive input shape must override.
                        let config = $moduleName::Config::builder()
                            .endpoint_url("http://localhost:1234")
                            .http_client(http_client.clone())
                            .emit_input_attributes(["bucket"])
                            .interceptor(observer.clone())
                            .build();
                        let client = $moduleName::Client::from_conf(config);
                        let _ = client.say_hello()
                            .bucket("example-bucket")
                            .send()
                            .await
                            .expect("success");

                        assert!(
                            !*observer.present.lock().unwrap(),
                            "a @sensitive input shape must capture nothing",
                        );
                    }
                    """,
                    *codegenScope(codegenContext.runtimeConfig),
                )
            }
        }
    }
}
