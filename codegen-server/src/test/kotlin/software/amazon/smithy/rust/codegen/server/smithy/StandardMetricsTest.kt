/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import org.junit.jupiter.api.Test
import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.ValueSource
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.ServerAdditionalSettings
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestVersion
import software.amazon.smithy.rust.codegen.server.smithy.testutil.ServerHttpTestHelpers
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.io.File

/** Standard server metrics (RFC 49), on both the generated-serde and schema-serde service paths. */
internal class StandardMetricsTest {
    private val model = File("../codegen-core/common-test-models/simple.smithy").readText().asSmithyModel()

    @ParameterizedTest
    @ValueSource(booleans = [false, true])
    fun `http 1 services wrap routing in MetricsService and honor enablement`(schemaSerde: Boolean) {
        serverIntegrationTest(
            model,
            IntegrationTestParams(
                additionalSettings = ServerAdditionalSettings.builder().schemaSerde(schemaSerde).toObjectNode(),
            ),
            testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
        ) { context, rustCrate ->
            check(context.usesSchemaHttpSerde == schemaSerde)
            val scope =
                arrayOf(
                    "Http" to RuntimeType.http(context.runtimeConfig),
                    "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                    "Tower" to ServerCargoDependency.tower(context).toType(),
                )
            rustCrate.testModule {
                unitTest("standard_metrics_disabled_by_default") {
                    rust("assert!(!crate::SimpleServiceConfig::builder().standard_metrics_enabled);")
                }

                unitTest("standard_metrics_enablement_survives_further_configuration") {
                    rustTemplate(
                        """
                        let builder = crate::SimpleServiceConfig::builder()
                            .enable_standard_metrics()
                            .layer(#{Tower}::layer::util::Identity::new())
                            .http_plugin(#{Server}::plugin::IdentityPlugin)
                            .model_plugin(#{Server}::plugin::IdentityPlugin);
                        assert!(builder.standard_metrics_enabled);
                        """,
                        *scope,
                    )
                }

                // A layer, registered through the config, that records each request's `on_finish` callback
                // invocation: whether it received metrics.
                rustTemplate(
                    """
                    ##[derive(Clone, Default)]
                    struct RecordLayer(::std::sync::Arc<::std::sync::Mutex<Vec<bool>>>);
                    impl<S> #{Tower}::Layer<S> for RecordLayer {
                        type Service = Record<S>;
                        fn layer(&self, inner: S) -> Record<S> {
                            Record { inner, log: self.0.clone() }
                        }
                    }
                    ##[derive(Clone)]
                    struct Record<S> {
                        inner: S,
                        log: ::std::sync::Arc<::std::sync::Mutex<Vec<bool>>>,
                    }
                    impl<S, B> #{Tower}::Service<#{Http}::Request<B>> for Record<S>
                    where
                        S: #{Tower}::Service<#{Http}::Request<B>>,
                    {
                        type Response = S::Response;
                        type Error = S::Error;
                        type Future = S::Future;
                        fn poll_ready(&mut self, cx: &mut ::std::task::Context<'_>) -> ::std::task::Poll<Result<(), S::Error>> {
                            self.inner.poll_ready(cx)
                        }
                        fn call(&mut self, mut req: #{Http}::Request<B>) -> Self::Future {
                            let log = self.log.clone();
                            #{Server}::metrics::RequestExt::on_finish(&mut req, move |metrics| {
                                log.lock().unwrap().push(metrics.is_some());
                            });
                            self.inner.call(req)
                        }
                    }
                    """,
                    *scope,
                )

                for (enabled in listOf(false, true)) {
                    tokioTest("config_layer_callback_runs_once_response_is_sent_with_metrics_${if (enabled) "enabled" else "disabled"}") {
                        rustTemplate(
                            """
                            let record = RecordLayer::default();
                            let config = crate::SimpleServiceConfig::builder()
                                .layer(record.clone())
                                ${if (enabled) ".enable_standard_metrics()" else ""}
                                .build();
                            let app = crate::SimpleService::builder(config)
                                .operation(|input: crate::input::OperationInput| async {
                                    crate::output::OperationOutput { message: input.message }
                                })
                                .build()
                                .expect("service should build");
                            let request = #{Http}::Request::builder()
                                .method("POST")
                                .uri("/operation")
                                .header("Content-Type", "application/json")
                                .body(#{Server}::body::Body::empty())
                                .unwrap();
                            let response = #{Tower}::ServiceExt::oneshot(app, request).await.unwrap();
                            assert_eq!(response.status(), #{Http}::StatusCode::OK);
                            assert!(record.0.lock().unwrap().is_empty(), "response body not sent yet");
                            #{ReadResponse}
                            let _ = bytes;
                            assert_eq!(*record.0.lock().unwrap(), [$enabled]);
                            """,
                            *scope,
                            "ReadResponse" to ServerHttpTestHelpers.httpBodyToBytes(context.runtimeConfig, "bytes", "response"),
                        )
                    }
                }

                for (enabled in listOf(false, true)) {
                    tokioTest("routes_requests_with_standard_metrics_${if (enabled) "enabled" else "disabled"}") {
                        rustTemplate(
                            """
                            let config = crate::SimpleServiceConfig::builder()
                                ${if (enabled) ".enable_standard_metrics()" else ""}
                                .build();
                            let app = crate::SimpleService::builder(config)
                                .operation(|input: crate::input::OperationInput| async {
                                    crate::output::OperationOutput { message: input.message }
                                })
                                .build()
                                .expect("service should build");
                            let _: &crate::SimpleService<#{Server}::metrics::MetricsService<_>> = &app;
                            let request = #{Http}::Request::builder()
                                .method("POST")
                                .uri("/operation")
                                .header("Content-Type", "application/json")
                                .body(#{Server}::body::Body::empty())
                                .unwrap();
                            let response = #{Tower}::ServiceExt::oneshot(app, request).await.unwrap();
                            assert_eq!(response.status(), #{Http}::StatusCode::OK);
                            """,
                            *scope,
                        )
                    }
                }
            }
        }
    }

    @Test
    fun `http 0 services do not support standard metrics`() {
        serverIntegrationTest(model, testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_0_X)) { context, _ ->
            check(!context.supportsStandardMetrics)
        }.forEach { generatedServer ->
            val sources =
                generatedServer.path.resolve("src").toFile().walk()
                    .filter { it.isFile && it.extension == "rs" }
                    .joinToString("\n") { it.readText() }
            assert(!sources.contains("standard_metrics")) { "http 0.x service mentions standard metrics" }
            assert(!sources.contains("MetricsService")) { "http 0.x service mentions MetricsService" }
        }
    }
}
