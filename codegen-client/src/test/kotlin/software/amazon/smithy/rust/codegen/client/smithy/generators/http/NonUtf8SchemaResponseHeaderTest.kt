/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators.http

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customizations.SchemaSerdeAllowlist
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest

/** End-to-end coverage for non-UTF-8 response headers on the schema-exclusive client path. */
class NonUtf8SchemaResponseHeaderTest {
    private val model =
        """
        namespace smithy.rust.codegen.test.schemaheaders

        use aws.protocols#restJson1

        @restJson1
        service HeaderTestService {
            version: "2023-01-01",
            operations: [Buffered, Streaming, Prefix, ErrorOperation],
        }

        @http(uri: "/buffered", method: "GET")
        operation Buffered {
            output: BufferedOutput,
        }

        structure BufferedOutput {
            @httpHeader("x-header")
            header: String,

            @httpHeader("x-int-list")
            intList: IntList,
        }

        list IntList { member: Integer }

        @http(uri: "/streaming", method: "GET")
        operation Streaming {
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
        operation Prefix {
            output: PrefixOutput,
        }

        structure PrefixOutput {
            @httpPrefixHeaders("X-Meta-")
            metadata: StringMap,
        }

        map StringMap { key: String, value: String }

        @http(uri: "/error", method: "GET")
        operation ErrorOperation {
            output: EmptyOutput,
            errors: [HeaderError],
        }

        structure EmptyOutput {}

        @error("client")
        @httpError(400)
        structure HeaderError {
            @httpHeader("x-error-header")
            errorHeader: String,
        }
        """.asSmithyModel()

    @Test
    fun `schema response policy reaches buffered streaming prefix and error paths`() {
        clientIntegrationTest(model) { codegenContext, rustCrate ->
            check(SchemaSerdeAllowlist.usesSchemaSerdeExclusively(codegenContext)) {
                "dedicated fixture namespace must exercise the schema-exclusive response path"
            }
            rustCrate.testModule {
                skipInterceptor(codegenContext)(this)

                tokioTest("schema_default_rejects_modeled_headers_on_every_call_site") {
                    rustTemplate(
                        """
                        let buffered_response = |_: #{http_1x}::Request<#{SdkBody}>| {
                            #{http_1x}::Response::builder()
                                .status(200)
                                .header("x-header", #{http_1x}::HeaderValue::from_bytes(b"value-\xe9").unwrap())
                                .body(#{SdkBody}::from(""))
                                .unwrap()
                        };
                        let buffered = crate::Client::from_conf(
                            crate::Config::builder()
                                .http_client(#{infallible_client_fn}(buffered_response))
                                .endpoint_url("http://localhost:1234")
                                .build(),
                        );
                        let err = buffered.buffered().send().await.unwrap_err();
                        let message = #{DisplayErrorContext}(&err).to_string();
                        assert!(message.contains("header"), "{message}");
                        assert!(message.contains("x-header"), "{message}");

                        let streaming_response = |_: #{http_1x}::Request<#{SdkBody}>| {
                            #{http_1x}::Response::builder()
                                .status(200)
                                .header("x-header", #{http_1x}::HeaderValue::from_bytes(b"value-\xe9").unwrap())
                                .body(#{SdkBody}::from(""))
                                .unwrap()
                        };
                        let streaming = crate::Client::from_conf(
                            crate::Config::builder()
                                .http_client(#{infallible_client_fn}(streaming_response))
                                .endpoint_url("http://localhost:1234")
                                .build(),
                        );
                        let err = streaming.streaming().send().await.unwrap_err();
                        let message = #{DisplayErrorContext}(&err).to_string();
                        assert!(message.contains("header"), "{message}");
                        assert!(message.contains("x-header"), "{message}");

                        let error_response = |_: #{http_1x}::Request<#{SdkBody}>| {
                            #{http_1x}::Response::builder()
                                .status(400)
                                .header("x-amzn-errortype", "HeaderError")
                                .header(
                                    "x-error-header",
                                    #{http_1x}::HeaderValue::from_bytes(b"value-\xe9").unwrap(),
                                )
                                .body(#{SdkBody}::from(""))
                                .unwrap()
                        };
                        let error_client = crate::Client::from_conf(
                            crate::Config::builder()
                                .http_client(#{infallible_client_fn}(error_response))
                                .endpoint_url("http://localhost:1234")
                                .build(),
                        );
                        let err = error_client.error_operation().send().await.unwrap_err();
                        let message = #{DisplayErrorContext}(&err).to_string();
                        assert!(message.contains("error_header"), "{message}");
                        assert!(message.contains("x-error-header"), "{message}");
                        """,
                        *scope(codegenContext),
                    )
                }

                tokioTest("schema_skip_is_whole_member_and_preserves_octets") {
                    rustTemplate(
                        """
                        let buffered_response = |_: #{http_1x}::Request<#{SdkBody}>| {
                            #{http_1x}::Response::builder()
                                .status(200)
                                .header("x-header", #{http_1x}::HeaderValue::from_bytes(b"value-\xe9").unwrap())
                                .body(#{SdkBody}::from(""))
                                .unwrap()
                        };
                        let interceptor = SkipNonUtf8Headers::default();
                        let buffered = crate::Client::from_conf(
                            crate::Config::builder()
                                .http_client(#{infallible_client_fn}(buffered_response))
                                .endpoint_url("http://localhost:1234")
                                .interceptor(interceptor.clone())
                                .build(),
                        );
                        let output = buffered.buffered().send().await.unwrap();
                        assert_eq!(output.header(), None);
                        assert_eq!(
                            interceptor.seen_after_deser(),
                            vec![("x-header".to_string(), b"value-\xe9".to_vec())],
                        );

                        let malformed_response = |_: #{http_1x}::Request<#{SdkBody}>| {
                            #{http_1x}::Response::builder()
                                .status(200)
                                .header("x-int-list", "not-an-integer")
                                .body(#{SdkBody}::from(""))
                                .unwrap()
                        };
                        let malformed = crate::Client::from_conf(
                            crate::Config::builder()
                                .http_client(#{infallible_client_fn}(malformed_response))
                                .endpoint_url("http://localhost:1234")
                                .interceptor(SkipNonUtf8Headers::default())
                                .build(),
                        );
                        malformed
                            .buffered()
                            .send()
                            .await
                            .expect_err("Skip must not hide readable malformed values");

                        let streaming_response = |_: #{http_1x}::Request<#{SdkBody}>| {
                            #{http_1x}::Response::builder()
                                .status(200)
                                .header("x-header", #{http_1x}::HeaderValue::from_bytes(b"value-\xe9").unwrap())
                                .body(#{SdkBody}::from("stream body"))
                                .unwrap()
                        };
                        let streaming = crate::Client::from_conf(
                            crate::Config::builder()
                                .http_client(#{infallible_client_fn}(streaming_response))
                                .endpoint_url("http://localhost:1234")
                                .interceptor(SkipNonUtf8Headers::default())
                                .build(),
                        );
                        assert_eq!(streaming.streaming().send().await.unwrap().header(), None);

                        let prefix_response = |_: #{http_1x}::Request<#{SdkBody}>| {
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
                        let prefix_interceptor = SkipNonUtf8Headers::default();
                        let prefix = crate::Client::from_conf(
                            crate::Config::builder()
                                .http_client(#{infallible_client_fn}(prefix_response))
                                .endpoint_url("http://localhost:1234")
                                .interceptor(prefix_interceptor.clone())
                                .build(),
                        );
                        let output = prefix.prefix().send().await.unwrap();
                        assert_eq!(output.metadata(), None);
                        assert_eq!(
                            prefix_interceptor.seen_after_deser(),
                            vec![("x-meta-bad".to_string(), b"value-\xe9".to_vec())],
                        );

                        let error_response = |_: #{http_1x}::Request<#{SdkBody}>| {
                            #{http_1x}::Response::builder()
                                .status(400)
                                .header("x-amzn-errortype", "HeaderError")
                                .header(
                                    "x-error-header",
                                    #{http_1x}::HeaderValue::from_bytes(b"value-\xe9").unwrap(),
                                )
                                .body(#{SdkBody}::from(""))
                                .unwrap()
                        };
                        let error_client = crate::Client::from_conf(
                            crate::Config::builder()
                                .http_client(#{infallible_client_fn}(error_response))
                                .endpoint_url("http://localhost:1234")
                                .interceptor(SkipNonUtf8Headers::default())
                                .build(),
                        );
                        let err = error_client.error_operation().send().await.unwrap_err();
                        match err.into_service_error() {
                            crate::operation::error_operation::ErrorOperationError::HeaderError(inner) => {
                                assert_eq!(inner.error_header(), None)
                            }
                            other => panic!("expected HeaderError after Skip, got {other:?}"),
                        }
                        """,
                        *scope(codegenContext),
                    )
                }
            }
        }
    }

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
                    fn name(&self) -> &'static str { "SkipNonUtf8Headers" }

                    fn read_before_execution(
                        &self,
                        _context: &#{BeforeSerializationRef}<'_>,
                        cfg: &mut #{ConfigBag},
                    ) -> Result<(), #{BoxError}> {
                        cfg.interceptor_state().store_put(#{NonUtf8HeaderHandling}::Skip);
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
        val runtimeApi = RuntimeType.smithyRuntimeApi(rc)
        return arrayOf(
            *RuntimeType.preludeScope,
            "AfterDeserializationRef" to
                runtimeApi.resolve("client::interceptors::context::AfterDeserializationInterceptorContextRef"),
            "BeforeSerializationRef" to
                runtimeApi.resolve("client::interceptors::context::BeforeSerializationInterceptorContextRef"),
            "BoxError" to RuntimeType.boxError(rc),
            "ConfigBag" to RuntimeType.configBag(rc),
            "DisplayErrorContext" to RuntimeType.smithyTypes(rc).resolve("error::display::DisplayErrorContext"),
            "Intercept" to runtimeApi.resolve("client::interceptors::Intercept"),
            "NonUtf8HeaderHandling" to runtimeApi.resolve("http::NonUtf8HeaderHandling"),
            "RuntimeComponents" to runtimeApi.resolve("client::runtime_components::RuntimeComponents"),
            "SdkBody" to RuntimeType.sdkBody(rc),
            "http_1x" to CargoDependency.Http1x.toType(),
            "infallible_client_fn" to
                CargoDependency.smithyHttpClientTestUtil(rc).toType().resolve("test_util::infallible_client_fn"),
        )
    }
}
