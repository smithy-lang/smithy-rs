/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.protocols

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import kotlin.io.path.readText

internal class RestJsonTest {
    val model =
        """
        namespace test
        use aws.protocols#restJson1
        use aws.api#service
        use smithy.test#httpRequestTests
        use smithy.test#httpResponseTests

        /// A REST JSON service that sends JSON requests and responses.
        @service(sdkId: "Rest Json Protocol")
        @restJson1
        service RestJsonExtras {
            version: "2019-12-16",
            operations: [StringPayload]
        }

        @http(uri: "/StringPayload", method: "POST")
        operation StringPayload {
            input: StringPayloadInput,
            output: StringPayloadInput
        }

        structure StringPayloadInput {
            payload: String,
            a: String,
            b: Integer
        }
        """.asSmithyModel()

    private val inputUnionWithEmptyStructure =
        """
        namespace test
        use aws.protocols#restJson1
        use aws.api#service

        @service(sdkId: "Rest Json Empty Struct")
        @restJson1
        service RestJsonEmptyStruct {
            version: "2019-12-16",
            operations: [TestOp]
        }

        @http(uri: "/test", method: "POST")
        operation TestOp {
            input: TestInput
        }

        structure TestInput {
            testUnion: TestUnion
        }

        union TestUnion {
            // Empty struct - RestJson ALWAYS uses inner variable, no warning
            emptyStruct: EmptyStruct,

            // Normal struct - RestJson uses inner variable
            normalStruct: NormalStruct
        }

        structure EmptyStruct {}

        structure NormalStruct {
            value: String
        }
        """.asSmithyModel()

    private val blobPayloadModel =
        """
        ${'$'}version: "2"
        namespace test

        use aws.protocols#restJson1

        @restJson1
        service BlobPayloadService {
            version: "2019-12-16",
            operations: [PutBlob]
        }

        @http(uri: "/blob", method: "POST")
        operation PutBlob {
            input: PutBlobInput
        }

        structure PutBlobInput {
            @httpPayload
            data: Blob
        }
        """.asSmithyModel()

    @Test
    fun `generate a rest json service that compiles`() {
        val testDir = clientIntegrationTest(model) { _, _ -> }

        // test the generated metadata
        val cargoToml = testDir.resolve("Cargo.toml").readText()
        assert(cargoToml.contains("codegen-version =")) { cargoToml }
        assert(cargoToml.contains("protocol = \"aws.protocols#restJson1\"")) { cargoToml }
    }

    @Test
    fun `union with empty struct always uses inner variable`() {
        // This test documents that RestJson protocol is immune to unused variable issues.
        // Unlike RestXml/AwsQuery, RestJson serializers always reference the inner variable
        // even for empty structs, so no underscore prefix is needed.
        // This test passes without any code changes, proving RestJson immunity.
        clientIntegrationTest(inputUnionWithEmptyStructure) { _, _ -> }
    }

    @Test
    fun `blob payload serialization reuses Bytes allocation`() {
        clientIntegrationTest(blobPayloadModel) { context, rustCrate ->
            rustCrate.testModule {
                tokioTest("blob_payload_reuses_bytes") {
                    rustTemplate(
                        """
                        let payload = #{Bytes}::from_static(b"hello, world!");
                        let payload_ptr = payload.as_ptr();
                        let (http_client, rx) = #{capture_request}(#{None});
                        let config = crate::Config::builder()
                            .http_client(http_client)
                            .endpoint_url("http://localhost:1234")
                            .behavior_version_latest()
                            .build();
                        let client = crate::Client::from_conf(config);

                        let _ = client
                            .put_blob()
                            .data(::aws_smithy_types::Blob::from_maybe_shared(payload))
                            .send()
                            .await;

                        let request = rx.expect_request();
                        let body = request.body().bytes().expect("in-memory request body");
                        assert_eq!(b"hello, world!", body);
                        assert_eq!(payload_ptr, body.as_ptr());
                        """,
                        *RuntimeType.preludeScope,
                        "Bytes" to RuntimeType.Bytes,
                        "capture_request" to RuntimeType.captureRequest(context.runtimeConfig),
                    )
                }
            }
        }
    }
}
