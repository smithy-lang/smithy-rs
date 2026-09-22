/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators.http

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest

/**
 * A service may echo back a header value that is not valid UTF-8 — HTTP permits any octet except a
 * control character in a header value. These tests pin that behavior:
 *
 *  - by default, a value bound to a modeled member is an error naming that member, rather than being
 *    silently dropped. Covered for `@httpHeader` and `@httpPrefixHeaders`, on both the buffered and
 *    the streaming deserializer paths.
 *  - `NonUtf8HeaderHandling::Skip` deserializes the member as if the header were absent, leaving the
 *    header on the response so the octets stay readable. For `@httpPrefixHeaders` that means the
 *    whole map is `None`, not a map missing one entry.
 *  - alternatively a caller can remove the header outright from an interceptor, which needs no
 *    setting at all — only the byte accessors on `Headers`.
 */
class NonUtf8ResponseHeaderTest {
    private val model =
        """
        namespace test

        use aws.protocols#restJson1

        @restJson1
        service TestService {
            version: "2023-01-01",
            operations: [SomeOperation, StreamingOperation, PrefixOperation],
        }

        @http(uri: "/", method: "GET")
        operation SomeOperation {
            output: SomeOutput,
        }

        structure SomeOutput {
            @httpHeader("x-header")
            header: String,
        }

        // A streaming output takes a different deserializer path than a buffered one, so it needs
        // its own coverage.
        @http(uri: "/streaming", method: "GET")
        operation StreamingOperation {
            output: StreamingOutput,
        }

        structure StreamingOutput {
            @httpHeader("x-header")
            header: String,

            @httpPayload
            @required
            data: StreamingBlob,
        }

        @streaming
        blob StreamingBlob

        @http(uri: "/prefix", method: "GET")
        operation PrefixOperation {
            output: PrefixOutput,
        }

        structure PrefixOutput {
            @httpPrefixHeaders("x-meta-")
            metadata: StringMap,
        }

        map StringMap {
            key: String,
            value: String,
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
                skipInterceptor(codegenContext)(this)

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
                            vec![("x-header".to_string(), b"value-\xe9".to_vec())],
                            interceptor.seen_after_deser(),
                        );
                        """,
                        *scope(codegenContext),
                    )
                }
            }
        }
    }

    /**
     * A streaming output is deserialized by `deserialize_streaming_with_config` rather than the
     * buffered path, so `Skip` needs its own coverage there.
     */
    @Test
    fun skipAppliesOnTheStreamingDeserializerPath() {
        clientIntegrationTest(model) { codegenContext, rustCrate ->
            rustCrate.testModule {
                skipInterceptor(codegenContext)(this)

                tokioTest("skip_applies_on_the_streaming_deserializer_path") {
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
                            .streaming_operation()
                            .send()
                            .await
                            .expect("Skip applies to the streaming path too");

                        assert_eq!(None, out.header());
                        assert_eq!(
                            vec![("x-header".to_string(), b"value-\xe9".to_vec())],
                            interceptor.seen_after_deser(),
                        );
                        """,
                        *scope(codegenContext),
                    )
                }

                tokioTest("streaming_non_utf8_header_is_an_error_by_default") {
                    rustTemplate(
                        """
                        $nonUtf8Response
                        let client = crate::Client::from_conf(
                            crate::Config::builder()
                                .http_client(#{infallible_client_fn}(response))
                                .endpoint_url("http://localhost:1234")
                                .build(),
                        );

                        client
                            .streaming_operation()
                            .send()
                            .await
                            .expect_err("a non-UTF-8 header value must not be silently dropped");
                        """,
                        *scope(codegenContext),
                    )
                }
            }
        }
    }

    /**
     * For `@httpPrefixHeaders`, `Skip` drops the whole map rather than the offending entry. A
     * partially populated map would read as complete and hide what was dropped; `None` says plainly
     * that the map could not be built, and the octets of every entry are still recoverable.
     */
    @Test
    fun skipDropsTheEntirePrefixHeaderMap() {
        clientIntegrationTest(model) { codegenContext, rustCrate ->
            rustCrate.testModule {
                skipInterceptor(codegenContext)(this)

                tokioTest("skip_drops_the_entire_prefix_header_map") {
                    rustTemplate(
                        """
                        // One readable entry alongside one that is not valid UTF-8.
                        let response = |_: #{http_1x}::Request<#{SdkBody}>| {
                            #{http_1x}::Response::builder()
                                .status(200)
                                .header("x-meta-good", "readable")
                                .header(
                                    "x-meta-bad",
                                    #{http_1x}::HeaderValue::from_bytes(b"value-\xe9").unwrap(),
                                )
                                .body(#{SdkBody}::from(""))
                                .unwrap()
                        };

                        let interceptor = SkipNonUtf8Headers::default();
                        let client = crate::Client::from_conf(
                            crate::Config::builder()
                                .http_client(#{infallible_client_fn}(response))
                                .endpoint_url("http://localhost:1234")
                                .interceptor(interceptor.clone())
                                .build(),
                        );

                        let out = client
                            .prefix_operation()
                            .send()
                            .await
                            .expect("Skip tolerates the unreadable entry");

                        // Not a map containing only `good` — the whole member is absent.
                        assert_eq!(None, out.metadata());

                        assert_eq!(
                            vec![("x-meta-bad".to_string(), b"value-\xe9".to_vec())],
                            interceptor.seen_after_deser(),
                        );
                        """,
                        *scope(codegenContext),
                    )
                }

                tokioTest("prefix_header_non_utf8_is_an_error_by_default") {
                    rustTemplate(
                        """
                        let response = |_: #{http_1x}::Request<#{SdkBody}>| {
                            #{http_1x}::Response::builder()
                                .status(200)
                                .header("x-meta-good", "readable")
                                .header(
                                    "x-meta-bad",
                                    #{http_1x}::HeaderValue::from_bytes(b"value-\xe9").unwrap(),
                                )
                                .body(#{SdkBody}::from(""))
                                .unwrap()
                        };
                        let client = crate::Client::from_conf(
                            crate::Config::builder()
                                .http_client(#{infallible_client_fn}(response))
                                .endpoint_url("http://localhost:1234")
                                .build(),
                        );

                        client
                            .prefix_operation()
                            .send()
                            .await
                            .expect_err("a non-UTF-8 prefix header value must not be silently dropped");
                        """,
                        *scope(codegenContext),
                    )
                }
            }
        }
    }

    /**
     * Emits an interceptor that opts into [NonUtf8HeaderHandling::Skip] and, separately, records
     * which header values were still unreadable on the response once deserialization finished.
     * `Skip` does not remove anything, so that list is what a caller would use to recover the
     * octets.
     */
    private fun skipInterceptor(codegenContext: ClientCodegenContext): Writable =
        writable {
            rustTemplate(
                """
                ##[derive(Clone, Debug, Default)]
                struct SkipNonUtf8Headers {
                    seen: std::sync::Arc<std::sync::Mutex<Vec<(String, Vec<u8>)>>>,
                }

                impl SkipNonUtf8Headers {
                    fn seen_after_deser(&self) -> Vec<(String, Vec<u8>)> {
                        self.seen.lock().unwrap().clone()
                    }
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
                        let mut seen: Vec<(String, Vec<u8>)> = context
                            .response()
                            .headers()
                            .iter_bytes()
                            .filter(|(_, value)| std::str::from_utf8(value).is_err())
                            .map(|(name, value)| (name.to_owned(), value.to_vec()))
                            .collect();
                        seen.sort();
                        *self.seen.lock().unwrap() = seen;
                        Ok(())
                    }
                }
                """,
                *scope(codegenContext),
            )
        }

    private fun scope(codegenContext: ClientCodegenContext): Array<Pair<String, Any>> {
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
