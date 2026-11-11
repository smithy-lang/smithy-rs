/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest

/**
 * End-to-end behavior of the per-service default read timeout in generated clients.
 *
 * These assert the *resolved* `TimeoutConfig` an operation actually sees, read out of the config bag
 * by an interceptor, rather than the presence of a string in generated source. The value has to
 * survive codegen, the `ServiceRuntimePlugin` layer, and the config-bag merge, and only the resolved
 * value proves all three.
 *
 * The scenarios are the three value buckets the artifact can produce, plus a Rust override:
 *
 * | `sdkId`    | artifact | expected        |
 * |------------|----------|-----------------|
 * | `dontcare` | absent   | 5 min (general) |
 * | `SQS`      | 900000   | 15 min          |
 * | `S3`       | -1       | none            |
 * | `Polly`    | -1       | 5 min, overridden |
 */
class DefaultReadTimeoutDecoratorTest {
    /** The shared SDK test model with a substituted `sdkId`, so one model shape covers every bucket. */
    private fun modelWithSdkId(sdkId: String) =
        """
        namespace test

        use aws.api#service
        use aws.auth#sigv4
        use aws.protocols#restJson1
        use smithy.rules#endpointRuleSet

        @service(sdkId: "$sdkId")
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

        @http(uri: "/SomeOperation", method: "GET")
        @optionalAuth
        operation SomeOperation {}
        """.asSmithyModel()

    private fun scope(rc: RuntimeConfig) =
        arrayOf(
            "tokio_test" to writable { Attribute.TokioTest.render(this) },
            "infallible_client_fn" to
                CargoDependency.smithyHttpClientTestUtil(rc)
                    .toType().resolve("test_util::infallible_client_fn"),
        )

    /**
     * Emits a harness that captures the resolved `TimeoutConfig` for one request and returns its
     * `read_timeout`, so each assertion below is a single comparison.
     */
    private fun RustWriter.harness(
        moduleName: String,
        rc: RuntimeConfig,
    ) {
        rustTemplate(
            """
            use aws_smithy_runtime_api::box_error::BoxError;
            use aws_smithy_runtime_api::client::interceptors::context::BeforeTransmitInterceptorContextRef;
            use aws_smithy_runtime_api::client::interceptors::Intercept;
            use aws_smithy_runtime_api::client::runtime_components::RuntimeComponents;
            use aws_smithy_types::body::SdkBody;
            use aws_smithy_types::config_bag::ConfigBag;
            use aws_smithy_types::timeout::TimeoutConfig;
            use std::sync::{Arc, Mutex};
            use std::time::Duration;
            use #{infallible_client_fn};

            ##[derive(Debug, Clone, Default)]
            struct CaptureTimeoutConfig {
                captured: Arc<Mutex<Option<TimeoutConfig>>>,
            }

            impl Intercept for CaptureTimeoutConfig {
                fn name(&self) -> &'static str {
                    "CaptureTimeoutConfig"
                }

                fn read_before_attempt(
                    &self,
                    _context: &BeforeTransmitInterceptorContextRef<'_>,
                    _runtime_components: &RuntimeComponents,
                    cfg: &mut ConfigBag,
                ) -> Result<(), BoxError> {
                    *self.captured.lock().unwrap() = cfg.load::<TimeoutConfig>().cloned();
                    Ok(())
                }
            }

            /// Sends one request and returns the `read_timeout` the HTTP client was actually handed.
            ///
            /// This is the last link in the chain: a resolved `TimeoutConfig` is only useful if it
            /// reaches the connector as `HttpConnectorSettings`. `http_client_fn` is given the
            /// settings the orchestrator built, so capturing there proves the whole path from the
            /// vendored artifact through codegen, the config layer, and the merge.
            async fn connector_read_timeout(
                builder: $moduleName::config::Builder,
            ) -> Option<Duration> {
                use aws_smithy_runtime_api::client::http::{http_client_fn, HttpClient};

                let inner = infallible_client_fn(|_req| {
                    ::http_1x::Response::builder().body(SdkBody::empty()).unwrap()
                });
                let captured: Arc<Mutex<Option<Option<Duration>>>> = Default::default();
                let sink = captured.clone();
                let http_client = http_client_fn(move |settings, components| {
                    *sink.lock().unwrap() = Some(settings.read_timeout());
                    inner.http_connector(settings, components)
                });

                let config = builder.http_client(http_client).build();
                let client = $moduleName::Client::from_conf(config);
                let _ = client.some_operation().send().await;
                let seen = captured.lock().unwrap().take();
                seen.expect("the HTTP client must have been asked for a connector")
            }

            /// Turns the opt-in on for this process.
            ///
            /// The gate is read when the client is constructed, so this has to run first. Every
            /// test in a given binary writes the same value, so the write cannot be observed
            /// racing; the unset case therefore needs a binary of its own rather than a flag flip.
            ///
            /// The harness is shared by every binary below, and the one covering the opt-in states
            /// sets the variable itself, so this is unused there.
            ##[allow(dead_code)]
            fn enable_opt_in() {
                std::env::set_var("AWS_ENABLE_DEFAULT_SOCKET_TIMEOUT_2026", "true");
            }

            /// Sends one request through `builder` and returns the resolved `TimeoutConfig`.
            async fn resolve(builder: $moduleName::config::Builder) -> TimeoutConfig {
                let capture = CaptureTimeoutConfig::default();
                let http_client = infallible_client_fn(|_req| {
                    ::http_1x::Response::builder().body(SdkBody::empty()).unwrap()
                });
                let config = builder
                    .http_client(http_client)
                    .interceptor(capture.clone())
                    .build();
                let client = $moduleName::Client::from_conf(config);
                let _ = client.some_operation().send().await;
                let captured = capture.captured.lock().unwrap().clone();
                captured.expect("the interceptor must have observed a TimeoutConfig")
            }
            """,
            *scope(rc),
        )
    }

    @Test
    fun `a service absent from the artifact gets the general default once opted in`() {
        awsSdkIntegrationTest(modelWithSdkId("dontcare")) { ctx, crate ->
            val moduleName = ctx.moduleUseName()
            crate.integrationTest("default_read_timeout_general") {
                harness(moduleName, ctx.runtimeConfig)
                rustTemplate(
                    """
                    #{tokio_test}
                    async fn general_default_is_five_minutes() {
                        enable_opt_in();
                        let resolved = resolve($moduleName::Config::builder()).await;
                        assert_eq!(
                            resolved.read_timeout(),
                            Some(Duration::from_secs(300)),
                            "a service with no artifact entry takes the 5-minute general default"
                        );
                        assert_eq!(
                            resolved.connect_timeout(),
                            Some(Duration::from_millis(3100)),
                            "the read default must not displace the connect default"
                        );
                    }

                    #{tokio_test}
                    async fn the_default_reaches_the_http_connector() {
                        enable_opt_in();
                        assert_eq!(
                            connector_read_timeout($moduleName::Config::builder()).await,
                            Some(Duration::from_secs(300)),
                            "the resolved default must arrive as HttpConnectorSettings::read_timeout"
                        );
                    }

                    #{tokio_test}
                    async fn a_caller_value_wins() {
                        enable_opt_in();
                        let resolved = resolve(
                            $moduleName::Config::builder().timeout_config(
                                TimeoutConfig::builder()
                                    .read_timeout(Duration::from_secs(7))
                                    .build(),
                            ),
                        )
                        .await;
                        assert_eq!(resolved.read_timeout(), Some(Duration::from_secs(7)));
                    }
                    """,
                    *scope(ctx.runtimeConfig),
                )
            }

            // Separate binary: this is the only case that needs the variable *unset*, and the
            // process environment cannot hold both states at once.
            crate.integrationTest("default_read_timeout_opt_in") {
                harness(moduleName, ctx.runtimeConfig)
                rustTemplate(
                    """
                    /// Every state of the opt-in, in one test.
                    ///
                    /// These cases are deliberately not separate `##[test]`s: the gate is process
                    /// state, and separate tests in one binary would run on parallel threads and
                    /// overwrite each other's value. One sequential test is the only way to observe
                    /// more than one state without pulling in a serialization crate.
                    #{tokio_test}
                    async fn the_opt_in_gates_the_default() {
                        // Unset: off, and the connect default survives.
                        std::env::remove_var("AWS_ENABLE_DEFAULT_SOCKET_TIMEOUT_2026");
                        let resolved = resolve($moduleName::Config::builder()).await;
                        assert_eq!(
                            resolved.read_timeout(),
                            None,
                            "the default must stay off until the caller opts in"
                        );
                        assert_eq!(
                            resolved.connect_timeout(),
                            Some(Duration::from_millis(3100)),
                            "leaving the read default off must not disturb the connect default"
                        );
                        assert_eq!(
                            connector_read_timeout($moduleName::Config::builder()).await,
                            None,
                            "and nothing may reach the connector either"
                        );

                        // Anything that is not `true` is ignored rather than rejected, so a typo
                        // cannot stop a client from being built.
                        for value in ["", "false", "1", "yes", "0", "truthy"] {
                            std::env::set_var("AWS_ENABLE_DEFAULT_SOCKET_TIMEOUT_2026", value);
                            assert_eq!(
                                resolve($moduleName::Config::builder()).await.read_timeout(),
                                None,
                                "{value:?} must not enable the default"
                            );
                        }

                        // Case-insensitive `true`.
                        for value in ["true", "True", "TRUE", "tRuE"] {
                            std::env::set_var("AWS_ENABLE_DEFAULT_SOCKET_TIMEOUT_2026", value);
                            assert_eq!(
                                resolve($moduleName::Config::builder()).await.read_timeout(),
                                Some(Duration::from_secs(300)),
                                "{value:?} must enable the default"
                            );
                        }
                    }
                    """,
                    *scope(ctx.runtimeConfig),
                )
            }
        }
    }

    @Test
    fun `a partially exempt service gets the fifteen minute tier`() {
        awsSdkIntegrationTest(modelWithSdkId("SQS")) { ctx, crate ->
            val moduleName = ctx.moduleUseName()
            crate.integrationTest("default_read_timeout_partial") {
                harness(moduleName, ctx.runtimeConfig)
                rustTemplate(
                    """
                    #{tokio_test}
                    async fn partial_tier_is_fifteen_minutes() {
                        enable_opt_in();
                        let resolved = resolve($moduleName::Config::builder()).await;
                        assert_eq!(
                            resolved.read_timeout(),
                            Some(Duration::from_secs(900)),
                            "SQS is listed at 900000ms in the artifact"
                        );
                    }

                    #{tokio_test}
                    async fn the_tier_reaches_the_http_connector() {
                        enable_opt_in();
                        assert_eq!(
                            connector_read_timeout($moduleName::Config::builder()).await,
                            Some(Duration::from_secs(900))
                        );
                    }
                    """,
                    *scope(ctx.runtimeConfig),
                )
            }
        }
    }

    @Test
    fun `a fully exempt service gets no read timeout`() {
        awsSdkIntegrationTest(modelWithSdkId("S3")) { ctx, crate ->
            val moduleName = ctx.moduleUseName()
            crate.integrationTest("default_read_timeout_exempt") {
                harness(moduleName, ctx.runtimeConfig)
                rustTemplate(
                    """
                    #{tokio_test}
                    async fn no_read_timeout_reaches_the_http_connector() {
                        enable_opt_in();
                        assert_eq!(
                            connector_read_timeout($moduleName::Config::builder()).await,
                            None,
                            "an exempt service must hand the connector no read timeout"
                        );
                    }

                    #{tokio_test}
                    async fn exempt_service_has_no_read_timeout() {
                        // With the opt-in on, so this proves exemption rather than a closed gate.
                        enable_opt_in();
                        let resolved = resolve($moduleName::Config::builder()).await;
                        assert_eq!(
                            resolved.read_timeout(),
                            None,
                            "S3 is fully exempt (-1) in the artifact"
                        );
                        // The exempt path must emit no layer entry at all. An empty TimeoutConfig
                        // would trip the `has_timeouts()` short-circuit in MergeTimeoutConfig and
                        // take the connect default down with it.
                        assert_eq!(
                            resolved.connect_timeout(),
                            Some(Duration::from_millis(3100)),
                            "exempting the read timeout must leave the connect default intact"
                        );
                    }
                    """,
                    *scope(ctx.runtimeConfig),
                )
            }
        }
    }

    @Test
    fun `an overridden service takes the general default despite being exempt in the artifact`() {
        awsSdkIntegrationTest(modelWithSdkId("Polly")) { ctx, crate ->
            val moduleName = ctx.moduleUseName()
            crate.integrationTest("default_read_timeout_override") {
                harness(moduleName, ctx.runtimeConfig)
                rustTemplate(
                    """
                    #{tokio_test}
                    async fn overridden_service_gets_the_general_default() {
                        enable_opt_in();
                        let resolved = resolve($moduleName::Config::builder()).await;
                        assert_eq!(
                            resolved.read_timeout(),
                            Some(Duration::from_secs(300)),
                            "Polly is -1 in the artifact but overridden to the general default"
                        );
                    }

                    #{tokio_test}
                    async fn the_override_reaches_the_http_connector() {
                        enable_opt_in();
                        assert_eq!(
                            connector_read_timeout($moduleName::Config::builder()).await,
                            Some(Duration::from_secs(300))
                        );
                    }
                    """,
                    *scope(ctx.runtimeConfig),
                )
            }
        }
    }
}
