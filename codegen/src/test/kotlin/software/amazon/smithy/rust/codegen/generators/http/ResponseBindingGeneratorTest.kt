/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.generators.http

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.knowledge.HttpBinding
import software.amazon.smithy.model.knowledge.HttpBindingIndex
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.rustlang.RustModule
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolConfig
import software.amazon.smithy.rust.codegen.smithy.generators.http.ResponseBindingGenerator
import software.amazon.smithy.rust.codegen.smithy.transformers.OperationNormalizer
import software.amazon.smithy.rust.codegen.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.testutil.renderWithModelBuilder
import software.amazon.smithy.rust.codegen.testutil.testProtocolConfig
import software.amazon.smithy.rust.codegen.testutil.testSymbolProvider
import software.amazon.smithy.rust.codegen.testutil.unitTest
import software.amazon.smithy.rust.codegen.util.outputShape

class ResponseBindingGeneratorTest {
    private val baseModel = """
            namespace smithy.example

            @idempotent
            @http(method: "PUT", uri: "/", code: 200)
            operation PutObject {
                output: PutObjectResponse
            }

            list Extras {
                member: Integer
            }

            list Dates {
                member: Timestamp
            }

            @mediaType("video/quicktime")
            string Video

            structure PutObjectResponse {
                // Sent in the X-Dates header
                @httpHeader("X-Dates")
                dateHeaderList: Dates,

                @httpHeader("X-Ints")
                intList: Extras,

                @httpHeader("X-MediaType")
                mediaType: Video,

                // Sent in the body
                data: Blob,

                // Sent in the body
                additional: String,
            }
        """.asSmithyModel()
    private val model = OperationNormalizer(baseModel).transformModel(
        inputBodyFactory = OperationNormalizer.NoBody,
        outputBodyFactory = OperationNormalizer.NoBody
    )
    private val operationShape = model.expectShape(ShapeId.from("smithy.example#PutObject"), OperationShape::class.java)
    private val symbolProvider = testSymbolProvider(model)
    private val testProtocolConfig: ProtocolConfig = testProtocolConfig(model)

    private fun RustWriter.renderOperation() {
        operationShape.outputShape(model).renderWithModelBuilder(model, symbolProvider, this)
        rustBlock("impl PutObjectOutput") {
            val bindings = HttpBindingIndex.of(model).getResponseBindings(operationShape, HttpBinding.Location.HEADER)
            bindings.forEach { binding ->

                val runtimeType = ResponseBindingGenerator(
                    testProtocolConfig, operationShape
                ).generateDeserializeHeaderFn(binding)
                // little hack to force these functions to be generated
                rust("// use #T;", runtimeType)
            }
        }
    }

    @Test
    fun deserializeHeadersIntoOutputShape() {
        val testProject = TestWorkspace.testProject(symbolProvider)
        testProject.withModule(RustModule.default("output", public = true)) {
            it.renderOperation()
            it.unitTest(
                """
                use crate::http_serde;
                let resp = http::Response::builder()
                    .header("X-Ints", "1,2,3")
                    .header("X-Ints", "4,5,6")
                    .header("X-MediaType", "c21pdGh5LXJz")
                    .header("X-Dates", "Mon, 16 Dec 2019 23:48:18 GMT")
                    .header("X-Dates", "Mon, 16 Dec 2019 23:48:18 GMT,Tue, 17 Dec 2019 23:48:18 GMT")
                    .body(()).expect("valid request");
                assert_eq!(http_serde::deser_header_put_object_int_list(resp.headers()).unwrap(), Some(vec![1,2,3,4,5,6]));
                assert_eq!(http_serde::deser_header_put_object_media_type(resp.headers()).expect("valid").unwrap(), "smithy-rs");
                assert_eq!(http_serde::deser_header_put_object_date_header_list(resp.headers()).unwrap().unwrap().len(), 3);
            """
            )
        }
        testProject.compileAndTest()
    }
}
