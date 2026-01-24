/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators.http

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.HttpTrait
import software.amazon.smithy.rust.codegen.client.testutil.testClientCodegenContext
import software.amazon.smithy.rust.codegen.client.testutil.testSymbolProvider
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.generators.operationBuildError
import software.amazon.smithy.rust.codegen.core.smithy.protocols.RestJson
import software.amazon.smithy.rust.codegen.core.smithy.transformers.OperationNormalizer
import software.amazon.smithy.rust.codegen.core.testutil.TestRuntimeConfig
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.core.testutil.renderWithModelBuilder
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.expectTrait

class RequestBindingGeneratorTest {
    private val baseModel =
        """
        namespace smithy.example

        @idempotent
        @http(method: "PUT", uri: "/{bucketName}/{key}", code: 200)
        operation PutObject {
            input: PutObjectRequest
        }

        list Extras {
            member: Integer
        }

        list Dates {
            member: Timestamp
        }

        @enum([
            {
                value: "variantA",
                name: "VARIANT_A",
            },
            {
                value: "variantB",
                name: "VARIANT_B",
            },
        ])
        string RequiredEnum

        @mediaType("video/quicktime")
        string Video

        map StringMap {
            key: String,
            value: String
        }

        structure PutObjectRequest {
            // Sent in the URI label named "key".
            @required
            @httpLabel
            key: Timestamp,

            // Sent in the URI label named "bucketName".
            @required
            @httpLabel
            bucketName: String,

            // Sent in the X-Dates header
            @httpHeader("X-Dates")
            dateHeaderList: Dates,

            @httpHeader("X-Ints")
            intList: Extras,

            @httpHeader("X-MediaType")
            mediaType: Video,

            // Sent in the query string as paramName
            @httpQuery("paramName")
            someValue: String,

            @httpQuery("primitive")
            primitive: PrimitiveInteger,

            @httpQuery("enabled")
            enabled: PrimitiveBoolean,

            @httpQuery("hello")
            extras: Extras,

            @required
            @httpQuery("uploadId")
            uploadId: String,

            // Sent in the body
            data: Blob,

            // Sent in the body
            additional: String,

            @httpPrefixHeaders("X-Prefix-")
            prefix: StringMap,

            @httpHeader("stringHeader")
            stringHeader: SensitiveStringHeader
        }

        @sensitive
        string SensitiveStringHeader
        """.asSmithyModel()
    private val model = OperationNormalizer.transform(baseModel)
    private val symbolProvider = testSymbolProvider(model)
    private val operationShape = model.expectShape(ShapeId.from("smithy.example#PutObject"), OperationShape::class.java)
    private val inputShape = model.expectShape(operationShape.input.get(), StructureShape::class.java)
    private val operationModule = symbolProvider.moduleForShape(inputShape)

    private fun renderOperation(rustCrate: RustCrate) {
        inputShape.renderWithModelBuilder(model, symbolProvider, rustCrate)
        rustCrate.withModule(operationModule) {
            val codegenContext = testClientCodegenContext(model)
            val bindingGen =
                RequestBindingGenerator(
                    codegenContext,
                    // Any protocol is fine for this test.
                    RestJson(codegenContext),
                    operationShape,
                )
            rustBlock("impl PutObjectInput") {
                // RequestBindingGenerator's functions expect to be rendered inside a function,
                // but the unit test needs to call some of these functions individually. This generates
                // some wrappers that can be called directly from the tests. The functions will get duplicated,
                // but that's not a problem.

                rustBlock(
                    "pub fn test_uri_query(&self, mut output: &mut String) -> Result<(), #T>",
                    TestRuntimeConfig.operationBuildError(),
                ) {
                    bindingGen.renderUpdateHttpBuilder(this)
                    rust("uri_query(self, output)")
                }

                rustBlock(
                    "pub fn test_uri_base(&self, mut output: &mut String) -> Result<(), #T>",
                    TestRuntimeConfig.operationBuildError(),
                ) {
                    bindingGen.renderUpdateHttpBuilder(this)
                    rust("uri_base(self, output)")
                }

                rustBlock(
                    "pub fn test_request_builder_base(&self) -> Result<#T, #T>",
                    RuntimeType.HttpRequestBuilder1x,
                    TestRuntimeConfig.operationBuildError(),
                ) {
                    bindingGen.renderUpdateHttpBuilder(this)
                    rust("let builder = #T::new();", RuntimeType.HttpRequestBuilder1x)
                    rust("update_http_builder(self, builder)")
                }
            }
        }
    }

    @Test
    fun `produces correct uri format strings`() {
        val httpTrait = operationShape.expectTrait<HttpTrait>()
        httpTrait.uriFormatString() shouldBe ("/{bucketName}/{key}".dq())
    }

    @Test
    fun `generates valid request bindings`() {
        val project = TestWorkspace.testProject(symbolProvider)
        renderOperation(project)
        project.withModule(operationModule) { // Currently rendering the operation renders the protocols—I want to separate that at some point.
            unitTest(
                name = "generate_uris",
                test = """
                    let ts = aws_smithy_types::DateTime::from_secs(10123125);
                    let inp = PutObjectInput::builder()
                        .bucket_name("somebucket/ok")
                        .key(ts.clone())
                        .upload_id("some-valid-id")
                        .set_extras(Some(vec![0,1,2,44]))
                        .some_value("svq!!%&")
                        .build().expect("build should succeed");
                    let mut o = String::new();
                    inp.test_uri_base(&mut o);
                    assert_eq!(o.as_str(), "/somebucket%2Fok/1970-04-28T03%3A58%3A45Z");
                    o.clear();
                    inp.test_uri_query(&mut o);
                    assert_eq!(o.as_str(), "?paramName=svq%21%21%25%26&hello=0&hello=1&hello=2&hello=44&uploadId=some-valid-id")
                """,
            )

            unitTest(
                name = "serialize_non_zero_values",
                test = """
                    let ts = aws_smithy_types::DateTime::from_secs(10123125);
                    let inp = PutObjectInput::builder()
                        .bucket_name("somebucket/ok")
                        .key(ts.clone())
                        .upload_id("some-valid-id")
                        .primitive(1)
                        .enabled(true)
                        .build().expect("build should succeed");
                    let mut o = String::new();
                    inp.test_uri_query(&mut o);
                    assert_eq!(o.as_str(), "?primitive=1&enabled=true&uploadId=some-valid-id")
                """,
            )

            unitTest(
                name = "build_http_requests",
                test = """
                    use std::collections::HashMap;
                    let ts = aws_smithy_types::DateTime::from_secs(10123125);
                    let inp = PutObjectInput::builder()
                        .bucket_name("buk")
                        .set_date_header_list(Some(vec![ts.clone()]))
                        .set_int_list(Some(vec![0,1,44]))
                        .key(ts.clone())
                        .upload_id("some-valid-id")
                        .set_extras(Some(vec![0,1]))
                        .some_value("qp")
                        .media_type("base64encodethis")
                        .prefix("k".to_string(), "😹".to_string())
                        .build().unwrap();
                    let http_request = inp.test_request_builder_base().expect("valid input").body(()).unwrap();
                    assert_eq!(http_request.uri(), "/buk/1970-04-28T03%3A58%3A45Z?paramName=qp&hello=0&hello=1&uploadId=some-valid-id");
                    assert_eq!(http_request.method(), "PUT");
                    let mut date_header = http_request.headers().get_all("X-Dates").iter();
                    assert_eq!(date_header.next().unwrap(), "Tue, 28 Apr 1970 03:58:45 GMT");
                    assert_eq!(date_header.next(), None);

                    let int_header = http_request.headers().get_all("X-Ints").iter().map(|hv|hv.to_str().unwrap()).collect::<Vec<_>>();
                    assert_eq!(int_header, vec!["0", "1", "44"]);

                    let base64_header = http_request.headers().get_all("X-MediaType").iter().map(|hv|hv.to_str().unwrap()).collect::<Vec<_>>();
                    assert_eq!(base64_header, vec!["YmFzZTY0ZW5jb2RldGhpcw=="]);

                    let prefix_header = http_request.headers().get_all("X-Prefix-k").iter().map(|hv|std::str::from_utf8(hv.as_ref()).unwrap()).collect::<Vec<_>>();
                    assert_eq!(prefix_header, vec!["😹"])
                """,
            )

            unitTest(
                name = "invalid_header_name_produces_error",
                test = """
                    use std::collections::HashMap;
                    let ts = aws_smithy_types::DateTime::from_secs(10123125);
                    let inp = PutObjectInput::builder()
                        .bucket_name("buk")
                        .key(ts.clone())
                        .upload_id("some-valid-id")
                        .prefix("😹".to_string(), "😹".to_string())
                        .build().unwrap();
                    let err = inp.test_request_builder_base().expect_err("can't make a header out of a cat emoji");
                    assert_eq!(format!("{}", err), "invalid field in input: prefix (details: `😹` cannot be used as a header name: invalid HTTP header name)");
                """,
            )

            unitTest(
                name = "invalid_prefix_header_value_produces_an_error",
                test = """
                    use std::collections::HashMap;
                    let ts = aws_smithy_types::DateTime::from_secs(10123125);
                    let inp = PutObjectInput::builder()
                        .bucket_name("buk")
                        .key(ts.clone())
                        .upload_id("some-valid-id")
                        .prefix("valid-key".to_string(), "\n can't put a newline in a header value".to_string())
                        .build().unwrap();
                    let err = inp.test_request_builder_base().expect_err("can't make a header with a newline");
                    assert_eq!(format!("{}", err), "invalid field in input: prefix (details: `\n can\'t put a newline in a header value` cannot be used as a header value: failed to parse header value)");
                """,
            )

            unitTest(
                name = "invalid_header_value_produces_an_error",
                test = """
                    let ts = aws_smithy_types::DateTime::from_secs(10123125);
                    let inp = PutObjectInput::builder()
                        .bucket_name("buk")
                        .key(ts.clone())
                        .upload_id("some-valid-id")
                        .string_header("\n is not valid")
                        .build().unwrap();
                    let err = inp.test_request_builder_base().expect_err("can't make a header with a newline");
                    // make sure we obey the sensitive trait
                    assert_eq!(format!("{}", err), "invalid field in input: string_header (details: `*** Sensitive Data Redacted ***` cannot be used as a header value: failed to parse header value)");
                """,
            )

            unitTest(
                name = "missing_uri_label_produces_an_error",
                test = """
                    let ts = aws_smithy_types::DateTime::from_secs(10123125);
                    let inp = PutObjectInput::builder()
                        // don't set bucket
                        // .bucket_name("buk")
                        .key(ts.clone())
                        .upload_id("some-valid-id")
                        .build().unwrap();
                    let err = inp.test_request_builder_base().expect_err("can't build request with bucket unset");
                    let message = err.to_string();
                    let expected = "bucket_name was missing: cannot be empty or unset";
                    assert!(message.contains(expected), "expected '{message}' to contain '{expected}'");
                """,
            )

            unitTest(
                name = "missing_timestamp_uri_label_produces_an_error",
                test = """
                    let ts = aws_smithy_types::DateTime::from_secs(10123125);
                    let inp = PutObjectInput::builder()
                        .bucket_name("buk")
                        // don't set key
                        // .key(ts.clone())
                        .upload_id("some-valid-id")
                        .build().unwrap();
                    let err = inp.test_request_builder_base().expect_err("can't build request with bucket unset");
                    let message = err.to_string();
                    let expected = "key was missing: cannot be empty or unset";
                    assert!(message.contains(expected), "expected '{message}' to contain '{expected}'");
                """,
            )

            unitTest(
                name = "empty_uri_label_produces_an_error",
                test = """
                    let ts = aws_smithy_types::DateTime::from_secs(10123125);
                    let inp = PutObjectInput::builder()
                        .bucket_name("")
                        .key(ts.clone())
                        .upload_id("some-valid-id")
                        .build().unwrap();
                    let err = inp.test_request_builder_base().expect_err("can't build request with bucket unset");
                    let message = err.to_string();
                    let expected = "bucket_name was missing: cannot be empty or unset";
                    assert!(message.contains(expected), "expected '{message}' to contain '{expected}'");
                """,
            )

            unitTest(
                name = "error_when_required_query_param_is_missing",
                test = """
                    let ts = aws_smithy_types::DateTime::from_secs(10123125);
                    let inp = PutObjectInput::builder()
                        .bucket_name("buk")
                        // don't set uploadId
                        // .upload_id("some-id")
                        .key(ts.clone())
                        .build().unwrap();
                    let err = inp.test_request_builder_base().expect_err("can't build request with uploadId unset");
                    let message = err.to_string();
                    let expected = "upload_id was missing: cannot be empty or unset";
                    assert!(message.contains(expected), "expected '{message}' to contain '{expected}'");
                """,
            )

            unitTest(
                name = "error_when_required_query_param_is_empty",
                test = """
                    let ts = aws_smithy_types::DateTime::from_secs(10123125);
                    let inp = PutObjectInput::builder()
                        .bucket_name("buk")
                        .key(ts.clone())
                        .upload_id("")
                        .build().unwrap();
                    let err = inp.test_request_builder_base().expect_err("can't build request with uploadId unset");
                    let message = err.to_string();
                    let expected = "upload_id was missing: cannot be empty or unset";
                    assert!(message.contains(expected), "expected '{message}' to contain '{expected}'");
                """,
            )
        }

        project.compileAndTest()
    }
}
