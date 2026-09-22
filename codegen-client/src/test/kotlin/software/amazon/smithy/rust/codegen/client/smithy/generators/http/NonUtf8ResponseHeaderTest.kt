/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators.http

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest

/**
 * A service may echo back a header value that is not valid UTF-8 — HTTP permits any octet except a
 * control character in a header value. These tests pin the two ends of that behavior:
 *
 *  - by default, a non-UTF-8 value bound to a modeled member is an error naming that member, rather
 *    than being silently dropped;
 *  - a caller that does not need the value can recover the raw octets and drop the header with an
 *    interceptor, after which the operation succeeds and the member deserializes to `None`.
 *
 * The second case is the supported workaround: it needs no codegen or configuration support, only
 * the byte accessors on `Headers`.
 */
class NonUtf8ResponseHeaderTest {
    private val model =
        """
        namespace test

        use aws.protocols#restJson1

        @restJson1
        service TestService {
            version: "2023-01-01",
            operations: [SomeOperation],
        }

        @http(uri: "/", method: "GET")
        operation SomeOperation {
            output: SomeOutput,
        }

        structure SomeOutput {
            @httpHeader("x-header")
            header: String,
        }
        """.asSmithyModel()

    /**
     * `b"value-\xe9"` — a lone 0xE9 is a valid HTTP header octet (obs-text per RFC 7230) but is not
     * valid UTF-8.
     */
    private val nonUtf8Response =
        """
        let response = |_: #{http_1x}::Request<#{SdkBody}>| {
            #{http_1x}::Response::builder()
                .status(200)
                .header(
                    "x-header",
                    #{http_1x}::HeaderValue::from_bytes(b"value-\xe9").unwrap(),
                )
                .body(#{SdkBody}::from(""))
                .unwrap()
        };
        """

    @Test
    fun nonUtf8HeaderIsAnErrorByDefault() {
        clientIntegrationTest(model) { codegenContext, rustCrate ->
            rustCrate.testModule {
                tokioTest("non_utf8_header_is_an_error_by_default") {
                    rustTemplate(
                        """
                        $nonUtf8Response
                        let client = crate::Client::from_conf(
                            crate::Config::builder()
                                .http_client(#{infallible_client_fn}(response))
                                .endpoint_url("http://localhost:1234")
                                .build(),
                        );

                        let err = client
                            .some_operation()
                            .send()
                            .await
                            .expect_err("a non-UTF-8 header value must not be silently dropped");

                        // The failure names the member it could not parse, rather than surfacing as
                        // a dispatch failure for the whole response.
                        let msg = format!("{}", #{DisplayErrorContext}(&err));
                        assert!(msg.contains("header"), "{msg}");
                        """,
                        *scope(codegenContext),
                    )
                }
            }
        }
    }

    @Test
    fun anInterceptorCanRecoverTheBytesAndDropTheHeader() {
        clientIntegrationTest(model) { codegenContext, rustCrate ->
            rustCrate.testModule {
                rustTemplate(
                    """
                    /// Captures response header values that are not valid UTF-8, then removes them so
                    /// the modeled members they are bound to deserialize to `None` instead of failing.
                    ##[derive(Clone, Debug, Default)]
                    struct NonUtf8Headers {
                        captured: std::sync::Arc<std::sync::Mutex<Vec<(String, Vec<u8>)>>>,
                    }

                    impl NonUtf8Headers {
                        fn captured(&self) -> Vec<(String, Vec<u8>)> {
                            self.captured.lock().unwrap().clone()
                        }
                    }

                    impl #{Intercept} for NonUtf8Headers {
                        fn name(&self) -> &'static str {
                            "NonUtf8Headers"
                        }

                        fn modify_before_deserialization(
                            &self,
                            context: &mut #{ContextMut}<'_>,
                            _runtime_components: &#{RuntimeComponents},
                            _cfg: &mut #{ConfigBag},
                        ) -> Result<(), #{BoxError}> {
                            let headers = context.response_mut().headers_mut();

                            // Overwrite rather than append, so the contents always describe the
                            // response about to be deserialized even after a retry.
                            let captured: Vec<(String, Vec<u8>)> = headers
                                .iter_bytes()
                                .filter(|(_, value)| std::str::from_utf8(value).is_err())
                                .map(|(name, value)| (name.to_owned(), value.to_vec()))
                                .collect();

                            for (name, _) in &captured {
                                headers.remove(name);
                            }
                            *self.captured.lock().unwrap() = captured;

                            Ok(())
                        }
                    }
                    """,
                    *scope(codegenContext),
                )

                tokioTest("an_interceptor_can_recover_the_bytes_and_drop_the_header") {
                    rustTemplate(
                        """
                        $nonUtf8Response
                        let non_utf8 = NonUtf8Headers::default();
                        let client = crate::Client::from_conf(
                            crate::Config::builder()
                                .http_client(#{infallible_client_fn}(response))
                                .endpoint_url("http://localhost:1234")
                                .interceptor(non_utf8.clone())
                                .build(),
                        );

                        let out = client
                            .some_operation()
                            .send()
                            .await
                            .expect("the header was dropped, so the operation succeeds");

                        // The member is absent rather than holding a re-encoded approximation.
                        assert_eq!(None, out.header());

                        // ...and the caller still has the octets exactly as they arrived.
                        assert_eq!(
                            vec![("x-header".to_string(), b"value-\xe9".to_vec())],
                            non_utf8.captured(),
                        );
                        """,
                        *scope(codegenContext),
                    )
                }
            }
        }
    }

    /**
     * `NonUtf8HeaderHandling::Skip` deserializes the member as if the header were absent, without
     * touching the response — so unlike the removal approach above, the octets are still readable
     * afterwards and any other reader of that header is unaffected.
     */
    @Test
    fun skipLeavesTheMemberAbsentAndTheOctetsReadable() {
        clientIntegrationTest(model) { codegenContext, rustCrate ->
            rustCrate.testModule {
                rustTemplate(
                    """
                    /// Opts into `Skip`, and separately records what the response still carried by the
                    /// time deserialization finished.
                    ##[derive(Clone, Debug, Default)]
                    struct SkipNonUtf8Headers {
                        seen_after_deser: std::sync::Arc<std::sync::Mutex<Option<Vec<u8>>>>,
                    }

                    impl #{Intercept} for SkipNonUtf8Headers {
                        fn name(&self) -> &'static str {
                            "SkipNonUtf8Headers"
                        }

                        fn read_before_execution(
                            &self,
                            _context: &#{BeforeSerializationRef}<'_>,
                            cfg: &mut #{ConfigBag},
                        ) -> Result<(), #{BoxError}> {
                            cfg.interceptor_state()
                                .store_put(#{NonUtf8HeaderHandling}::Skip);
                            Ok(())
                        }

                        fn read_after_deserialization(
                            &self,
                            context: &#{AfterDeserializationRef}<'_>,
                            _runtime_components: &#{RuntimeComponents},
                            _cfg: &mut #{ConfigBag},
                        ) -> Result<(), #{BoxError}> {
                            *self.seen_after_deser.lock().unwrap() = context
                                .response()
                                .headers()
                                .get_bytes("x-header")
                                .map(|v| v.to_vec());
                            Ok(())
                        }
                    }
                    """,
                    *scope(codegenContext),
                )

                tokioTest("skip_leaves_the_member_absent_and_the_octets_readable") {
                    rustTemplate(
                        """
                        $nonUtf8Response
                        let interceptor = SkipNonUtf8Headers::default();
                        let client = crate::Client::from_conf(
                            crate::Config::builder()
                                .http_client(#{infallible_client_fn}(response))
                                .endpoint_url("http://localhost:1234")
                                .interceptor(interceptor.clone())
                                .build(),
                        );

                        let out = client
                            .some_operation()
                            .send()
                            .await
                            .expect("Skip tolerates the unreadable value");

                        assert_eq!(None, out.header());

                        // The header was never removed, so the octets survived deserialization.
                        assert_eq!(
                            Some(b"value-\xe9".to_vec()),
                            *interceptor.seen_after_deser.lock().unwrap(),
                        );
                        """,
                        *scope(codegenContext),
                    )
                }
            }
        }
    }

    private fun scope(
        codegenContext: software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext,
    ): Array<Pair<String, Any>> {
        val rc = codegenContext.runtimeConfig
        val smithyRuntimeApi = RuntimeType.smithyRuntimeApi(rc)
        return arrayOf(
            *RuntimeType.preludeScope,
            "BoxError" to RuntimeType.boxError(rc),
            "ConfigBag" to RuntimeType.configBag(rc),
            "ContextMut" to
                smithyRuntimeApi.resolve(
                    "client::interceptors::context::BeforeDeserializationInterceptorContextMut",
                ),
            "AfterDeserializationRef" to
                smithyRuntimeApi.resolve(
                    "client::interceptors::context::AfterDeserializationInterceptorContextRef",
                ),
            "BeforeSerializationRef" to
                smithyRuntimeApi.resolve(
                    "client::interceptors::context::BeforeSerializationInterceptorContextRef",
                ),
            "DisplayErrorContext" to RuntimeType.smithyTypes(rc).resolve("error::display::DisplayErrorContext"),
            "Intercept" to smithyRuntimeApi.resolve("client::interceptors::Intercept"),
            "NonUtf8HeaderHandling" to smithyRuntimeApi.resolve("http::NonUtf8HeaderHandling"),
            "RuntimeComponents" to smithyRuntimeApi.resolve("client::runtime_components::RuntimeComponents"),
            "SdkBody" to RuntimeType.sdkBody(rc),
            "http_1x" to CargoDependency.Http1x.toType(),
            "infallible_client_fn" to
                CargoDependency.smithyHttpClientTestUtil(rc)
                    .toType().resolve("test_util::infallible_client_fn"),
        )
    }
}
