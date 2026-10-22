/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.server.smithy.protocols

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestVersion
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

internal class CborLiveBlobTest {
    @Test
    fun `live blob is serialized after ordinary generated members without collection`() {
        val model =
            """
            ${'$'}version: "2"
            namespace test.liveblob
            @smithy.protocols#rpcv2Cbor
            @aws.protocols#restJson1
            service Downloads { version: "1", operations: [Download] }
            @http(method: "POST", uri: "/download")
            operation Download {
                input := {}
                output := {
                    @httpHeader("x-title") title: String
                    @required @httpPayload fahad: StreamingBlob
                }
            }
            @streaming blob StreamingBlob
        """.asSmithyModel()
        serverIntegrationTest(
            model,
            IntegrationTestParams(
                additionalSettings = ObjectNode.parse("""{"codegen":{"schemaSerde":true}}""").expectObjectNode(),
                cargoCommand = "cargo test --quiet --lib --tests",
            ),
            testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
        ) { context, crate ->
            crate.testModule {
                tokioTest("live_blob") {
                    rustTemplate(
                        """
                        use #{BodyUtil}::BodyExt;
                        use #{Server}::operation::StreamingOperationShape;
                        let protocol = #{Server}::schema::SharedServerProtocol::metadata_routed(
                            #{Server}::schema::protocol::RpcV2Cbor::default());
                        let released = ::std::sync::Arc::new(::std::sync::atomic::AtomicBool::new(false));
                        let source_released = released.clone();
                        let mut sent = false;
                        let stream = #{Futures}::stream::poll_fn(move |_| {
                            if !source_released.load(::std::sync::atomic::Ordering::SeqCst) {
                                return ::std::task::Poll::Pending;
                            }
                            if sent { return ::std::task::Poll::Ready(#{None}); }
                            sent = true;
                            ::std::task::Poll::Ready(#{Some}(#{Ok}::<_, ::std::io::Error>(
                                #{HttpBody}::Frame::data(#{Bytes}::Bytes::from_static(b"hello")))))
                        });
                        // The explicit type assertion checks shared REST/CBOR output typing.
                        let streaming: #{Types}::byte_stream::ByteStream = #{Types}::byte_stream::ByteStream::new(
                            #{Types}::body::SdkBody::from_body_1_x(#{BodyUtil}::StreamBody::new(stream)));
                        let output = crate::output::DownloadOutput { title: #{Some}("hi".into()), fahad: streaming };
                        let response = crate::operation_shape::Download::serialize_streaming_output(output, &protocol);
                        assert_eq!(response.status(), 200);
                        assert!(!response.headers().contains_key("content-length"));
                        let mut body = response.into_body();
                        let prefix = body.frame().await.unwrap().unwrap().into_data().unwrap();
                        assert_eq!(prefix.as_ref(), b"\xbf\x65title\x62hi\x65fahad\x5f");
                        released.store(true, ::std::sync::atomic::Ordering::SeqCst);
                        assert_eq!(body.collect().await.unwrap().to_bytes().as_ref(), b"\x45hello\xff\xff");
                        let output = crate::output::DownloadOutput { title: #{None},
                            fahad: #{Types}::byte_stream::ByteStream::from_static(b"") };
                        let response = crate::operation_shape::Download::serialize_streaming_output(output, &protocol);
                        assert_eq!(response.into_body().collect().await.unwrap().to_bytes().as_ref(), b"\xbf\x65fahad\x5f\xff\xff");
                        """,
                        "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                        "Types" to RuntimeType.smithyTypes(context.runtimeConfig),
                        "BodyUtil" to CargoDependency.HttpBodyUtil01x.toType(),
                        "HttpBody" to CargoDependency.HttpBody1x.toType(),
                        "Bytes" to CargoDependency.Bytes.toType(),
                        "Futures" to CargoDependency.FuturesUtil.toType(),
                        *RuntimeType.preludeScope,
                    )
                }
            }
        }
    }
}
