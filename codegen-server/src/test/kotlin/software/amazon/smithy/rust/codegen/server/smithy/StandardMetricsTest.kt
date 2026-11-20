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
import software.amazon.smithy.rust.codegen.core.rustlang.writable
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

                recordLayer(scope)(this)

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
                            let record = record.0.lock().unwrap();
                            assert_eq!(record.len(), 1, "the callback runs once");
                            #{CheckMetrics}
                            """,
                            *scope,
                            "ReadResponse" to ServerHttpTestHelpers.httpBodyToBytes(context.runtimeConfig, "bytes", "response"),
                            "CheckMetrics" to
                                writable {
                                    if (enabled) {
                                        rust(
                                            """
                                            let metrics = record[0].as_ref().expect("metrics are enabled");
                                            assert_eq!(metrics.service().absolute(), "com.amazonaws.simple##SimpleService");
                                            assert_eq!(metrics.service_version(), None);
                                            """,
                                        )
                                    } else {
                                        rust("""assert!(record[0].is_none(), "metrics are disabled");""")
                                    }
                                },
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

    private val outcomeModel =
        """
        ${'$'}version: "2.0"
        namespace test.outcome
        use aws.protocols#restJson1
        @restJson1
        service OutcomeService { operations: [Classify] }
        @http(uri: "/classify", method: "POST")
        operation Classify {
            input := { kind: String }
            output := {}
            errors: [Rejected, Broken]
        }
        // Status codes that disagree with the classification, so that only the model can classify them.
        @error("client")
        @httpError(503)
        @suppress(["HttpResponseCodeSemantics"])
        structure Rejected { message: String }
        @error("server")
        @httpError(409)
        @suppress(["HttpResponseCodeSemantics"])
        structure Broken { message: String }
        """.asSmithyModel()

    @ParameterizedTest
    @ValueSource(booleans = [false, true])
    fun `outcome follows the model for operation results and the status otherwise`(schemaSerde: Boolean) {
        serverIntegrationTest(
            outcomeModel,
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
                recordLayer(scope)(this)
                tokioTest("outcomes") {
                    rustTemplate(
                        """
                        use #{Server}::metrics::{Fault, OutcomeSource};
                        for (body, expected_status, expected_result, expected_source, handler_ran) in [
                            (r##"{}"##, 200, "success", "operation", true),
                            (r##"{"kind":"rejected"}"##, 503, "client", "operation", true),
                            (r##"{"kind":"broken"}"##, 409, "server", "operation", true),
                            // A request the framework rejects before the handler runs: classified by status.
                            (r##"{"##, 400, "client", "internal", false),
                        ] {
                            let record = RecordLayer::default();
                            let config = crate::OutcomeServiceConfig::builder()
                                .layer(record.clone())
                                .layer(#{Server}::request::request_id::ServerRequestIdProviderLayer::new())
                                .enable_standard_metrics()
                                .build();
                            let app = crate::OutcomeService::builder(config)
                                .classify(|input: crate::input::ClassifyInput| async move {
                                    // Make the handler's execution time measurable.
                                    std::thread::sleep(std::time::Duration::from_millis(1));
                                    match input.kind.as_deref() {
                                        Some("rejected") => Err(crate::error::ClassifyError::Rejected(
                                            crate::error::Rejected { message: None },
                                        )),
                                        Some("broken") => Err(crate::error::ClassifyError::Broken(
                                            crate::error::Broken { message: None },
                                        )),
                                        _ => Ok(crate::output::ClassifyOutput {}),
                                    }
                                })
                                .build()
                                .expect("service should build");
                            let request = #{Http}::Request::builder()
                                .method("POST")
                                .uri("/classify")
                                .header("Content-Type", "application/json")
                                .body(#{Server}::body::to_boxed_sync(body))
                                .unwrap();
                            let response = #{Tower}::ServiceExt::oneshot(app, request).await.unwrap();
                            let status = response.status();
                            assert_eq!(status.as_u16(), expected_status, "{body}");
                            drop(response);
                            let record = record.0.lock().unwrap();
                            let metrics = record[0].as_ref().expect("metrics are enabled");
                            let outcome = metrics.outcome().expect("a response was produced");
                            // Outcomes deliberately do not implement `PartialEq`, so compare a summary.
                            let result = match &outcome.result {
                                Ok(_) => "success",
                                Err(error) => match error.fault {
                                    Fault::Client => "client",
                                    Fault::Server => "server",
                                    _ => "unknown",
                                },
                            };
                            let source = match outcome.source {
                                OutcomeSource::Operation => "operation",
                                OutcomeSource::Internal => "internal",
                                _ => "unknown",
                            };
                            assert_eq!((result, source), (expected_result, expected_source), "{body} ({status})");
                            // Routing chose the operation, even when the framework rejected the request before the handler ran.
                            let operation = metrics.operation().map(|id| id.absolute());
                            assert_eq!(operation, Some("test.outcome##Classify"), "{body} ({status})");
                            assert!(metrics.request_id().is_some(), "{body}: the request ID layer assigned an ID");
                            assert_eq!(metrics.status_code(), Some(status), "{body}");
                            let operation_duration = metrics.operation_duration();
                            if handler_ran {
                                assert!(operation_duration >= std::time::Duration::from_millis(1), "{body}: {operation_duration:?}");
                            } else {
                                assert_eq!(operation_duration, std::time::Duration::ZERO, "{body}");
                            }
                        }
                        """,
                        *scope,
                    )
                }
                tokioTest("layer_around_the_service_sees_unrouted_requests") {
                    rustTemplate(
                        """
                        use #{Server}::metrics::Fault;
                        let config = crate::OutcomeServiceConfig::builder().enable_standard_metrics().build();
                        let app = crate::OutcomeService::builder(config)
                            .classify(|_input: crate::input::ClassifyInput| async { Ok(crate::output::ClassifyOutput {}) })
                            .build()
                            .expect("service should build");
                        // Registers its callback before the request reaches the service.
                        let record = RecordLayer::default();
                        let app = #{Tower}::Layer::layer(&record, app);
                        let request = #{Http}::Request::builder()
                            .method("POST")
                            .uri("/no-such-operation")
                            .header("Content-Type", "application/json")
                            .body(#{Server}::body::Body::empty())
                            .unwrap();
                        let response = #{Tower}::ServiceExt::oneshot(app, request).await.unwrap();
                        assert_eq!(response.status(), #{Http}::StatusCode::NOT_FOUND);
                        drop(response);
                        let record = record.0.lock().unwrap();
                        assert_eq!(record.len(), 1, "the callback runs once");
                        let metrics = record[0].as_ref().expect("metrics are enabled");
                        assert!(metrics.operation().is_none(), "the request was not routed");
                        assert_eq!(metrics.status_code(), Some(#{Http}::StatusCode::NOT_FOUND));
                        let fault = metrics.outcome().and_then(|outcome| outcome.result.as_ref().err()).map(|error| error.fault.clone());
                        assert_eq!(fault, Some(Fault::Client));
                        """,
                        *scope,
                    )
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

    /**
     * A layer, registered through the config, that records the metrics received by each request's
     * `on_finish` callback. `scope` must bind `Http`, `Server` and `Tower`.
     */
    private fun recordLayer(scope: Array<out Pair<String, Any>>) =
        writable {
            rustTemplate(
                """
                type Log = ::std::sync::Arc<::std::sync::Mutex<Vec<Option<#{Server}::metrics::RequestMetrics>>>>;
                ##[derive(Clone, Default)]
                struct RecordLayer(Log);
                impl<S> #{Tower}::Layer<S> for RecordLayer {
                    type Service = Record<S>;
                    fn layer(&self, inner: S) -> Record<S> {
                        Record { inner, log: self.0.clone() }
                    }
                }
                ##[derive(Clone)]
                struct Record<S> {
                    inner: S,
                    log: Log,
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
                        #{Server}::metrics::RequestExt::on_finish(&mut req, move |metrics| log.lock().unwrap().push(metrics));
                        self.inner.call(req)
                    }
                }
                """,
                *scope,
            )
        }
}
