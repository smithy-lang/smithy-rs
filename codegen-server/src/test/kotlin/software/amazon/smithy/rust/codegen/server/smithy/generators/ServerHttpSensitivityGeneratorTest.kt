/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.pattern.UriPattern
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.traits.HttpHeaderTrait
import software.amazon.smithy.model.traits.HttpResponseCodeTrait
import software.amazon.smithy.model.traits.HttpTrait
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.withBlock
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverTestSymbolProvider
import software.amazon.smithy.rust.codegen.testutil.TestRuntimeConfig
import software.amazon.smithy.rust.codegen.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.testutil.unitTest
import software.amazon.smithy.rust.codegen.util.getTrait
import software.amazon.smithy.rust.codegen.util.inputShape

class ServerHttpSensitivityGeneratorTest {
    private val codegenScope = arrayOf(
        "SmithyHttpServer" to ServerCargoDependency.SmithyHttpServer(TestRuntimeConfig).asType(),
        "Http" to CargoDependency.Http.asType(),
    )

    @Test
    fun `find greedy label`() {
        val uri = "/pokemon-species/{name+}"
        val pattern = UriPattern.parse(uri)
        val position = findUriGreedyLabelPosition(pattern)!!
        assertEquals(position, 17)
    }

    @Test
    fun `find outer sensitive`() {
        val model = """
            namespace test

            operation Secret {
                input: Input,
            }

            @sensitive
            structure Input {
                @required
                @httpResponseCode
                code: Integer,
            }
        """.asSmithyModel()
        val operation = model.operationShapes.toList()[0]
        val generator = ServerHttpSensitivityGenerator(model, operation, TestRuntimeConfig)

        val inputShape = operation.inputShape(model)
        val members: List<String> = generator.findSensitiveBound<HttpResponseCodeTrait>(inputShape).map(MemberShape::getMemberName)

        assertEquals(members, listOf("code"))
    }

    @Test
    fun `find nested sensitive`() {
        val model = """
            namespace test

            operation Secret {
                input: Input,
            }

            @sensitive
            structure Input {
                @required
                @httpHeader("header-a")
                headerA: String,

                nested: Nested
            }

            structure Nested {
                @required
                @httpHeader("header-b")
                headerB: String
            }
        """.asSmithyModel()
        val operation = model.operationShapes.toList()[0]
        val generator = ServerHttpSensitivityGenerator(model, operation, TestRuntimeConfig)

        val inputShape = operation.inputShape(model)
        val members: List<String> = generator.findSensitiveBound<HttpHeaderTrait>(inputShape).map(MemberShape::getMemberName)

        assertEquals(members, listOf("headerB", "headerA"))
    }

    @Test
    fun `query closure`() {
        val model = """
            namespace test

            operation Secret {
                input: Input,
            }

            structure Input {
                @required
                @httpQuery("query_a")
                queryA: String,

                nestedB: NestedB
            }

           @sensitive
            structure NestedB {
                @required
                @httpQuery("query_c")
                queryC: String
            }
        """.asSmithyModel()
        val operation = model.operationShapes.toList()[0]
        val generator = ServerHttpSensitivityGenerator(model, operation, TestRuntimeConfig)

        val input = generator.input()!!
        val querySensitivity = generator.findQuerySensitivity(input)
        assertEquals(querySensitivity.allKeysSensitive, false)
        assertEquals((querySensitivity as ServerHttpSensitivityGenerator.QuerySensitivity.NotSensitiveMapValue).queryKeys, listOf("query_c"))

        val testProject = TestWorkspace.testProject(serverTestSymbolProvider(model))
        testProject.lib { writer ->
            writer.unitTest("query_closure") {
                withBlock("let closure = ", ";") {
                    generator.renderQueryClosure(writer, querySensitivity)
                }
                rustTemplate(
                    """
                    assert_eq!(closure("query_a"), #{SmithyHttpServer}::logging::sensitivity::uri::QueryMarker { key: false, value: false });
                    assert_eq!(closure("query_c"), #{SmithyHttpServer}::logging::sensitivity::uri::QueryMarker { key: false, value: true });
                    """,
                    *codegenScope,
                )
            }
        }
        testProject.compileAndTest()
    }

    @Test
    fun `query params closure`() {
        val model = """
            namespace test

            operation Secret {
                input: Input,
            }

            map StringMap {
                key: String,
                value: String
            }

            @sensitive
            structure Input {
                @required
                @httpQueryParams()
                params: StringMap,
            }
        """.asSmithyModel()
        val operation = model.operationShapes.toList()[0]
        val generator = ServerHttpSensitivityGenerator(model, operation, TestRuntimeConfig)

        val input = generator.input()!!
        val querySensitivity = generator.findQuerySensitivity(input)

        assertEquals(querySensitivity.allKeysSensitive, true)
        querySensitivity as ServerHttpSensitivityGenerator.QuerySensitivity.SensitiveMapValue

        val testProject = TestWorkspace.testProject(serverTestSymbolProvider(model))
        testProject.lib { writer ->
            writer.unitTest("query_params_closure") {
                withBlock("let closure = ", ";") {
                    generator.renderQueryClosure(writer, querySensitivity)
                }
                rustTemplate(
                    """
                    assert_eq!(closure("wildcard"), #{SmithyHttpServer}::logging::sensitivity::uri::QueryMarker { key: true, value: true });
                    """,
                    *codegenScope,
                )
            }
        }
        testProject.compileAndTest()
    }

    @Test
    fun `query params key closure`() {
        val model = """
            namespace test

            operation Secret {
                input: Input,
            }

            structure Input {
                @required
                @httpQueryParams()
                queryMap: QueryMap,
            }

            @sensitive
            string SensitiveKey

            map QueryMap {
                key: SensitiveKey,
                value: String
            }

        """.asSmithyModel()
        val operation = model.operationShapes.toList()[0]
        val generator = ServerHttpSensitivityGenerator(model, operation, TestRuntimeConfig)

        val input = generator.input()!!
        val querySensitivity = generator.findQuerySensitivity(input)
        assertEquals(querySensitivity.allKeysSensitive, true)
        assert((querySensitivity as ServerHttpSensitivityGenerator.QuerySensitivity.NotSensitiveMapValue).queryKeys.isEmpty())

        val testProject = TestWorkspace.testProject(serverTestSymbolProvider(model))
        testProject.lib { writer ->
            writer.unitTest("query_params_special_closure") {
                withBlock("let closure = ", ";") {
                    generator.renderQueryClosure(writer, querySensitivity)
                }
                rustTemplate(
                    """
                    assert_eq!(closure("wildcard"), #{SmithyHttpServer}::logging::sensitivity::uri::QueryMarker { key: true, value: false });
                    """,
                    *codegenScope,
                )
            }
        }
        testProject.compileAndTest()
    }

    @Test
    fun `query params value closure`() {
        val model = """
            namespace test

            operation Secret {
                input: Input,
            }

            structure Input {
                @required
                @httpQueryParams()
                queryMap: QueryMap,
            }

            @sensitive
            string SensitiveValue

            map QueryMap {
                key: String,
                value: SensitiveValue
            }

        """.asSmithyModel()
        val operation = model.operationShapes.toList()[0]
        val generator = ServerHttpSensitivityGenerator(model, operation, TestRuntimeConfig)

        val input = generator.input()!!
        val querySensitivity = generator.findQuerySensitivity(input)
        assertEquals(querySensitivity.allKeysSensitive, false)
        querySensitivity as ServerHttpSensitivityGenerator.QuerySensitivity.SensitiveMapValue

        val testProject = TestWorkspace.testProject(serverTestSymbolProvider(model))
        testProject.lib { writer ->
            writer.unitTest("query_params_special_closure") {
                withBlock("let closure = ", ";") {
                    generator.renderQueryClosure(writer, querySensitivity)
                }
                rustTemplate(
                    """
                    assert_eq!(closure("wildcard"), #{SmithyHttpServer}::logging::sensitivity::uri::QueryMarker { key: false, value: true });
                    """,
                    *codegenScope,
                )
            }
        }
        testProject.compileAndTest()
    }

    @Test
    fun `header closure`() {
        val model = """
            namespace test

            operation Secret {
                input: Input,
            }

            structure Input {
                @required
                @httpHeader("header-a")
                headerA: String,

                nestedB: NestedB
            }

            @sensitive
            structure NestedB {
                @required
                @httpHeader("header-c")
                headerC: String
            }
        """.asSmithyModel()
        val operation = model.operationShapes.toList()[0]
        val generator = ServerHttpSensitivityGenerator(model, operation, TestRuntimeConfig)

        val inputShape = operation.inputShape(model)
        val headerData = generator.findHeaderSensitivity(inputShape)
        assertEquals(headerData.headerKeys, listOf("header-c"))
        assertEquals((headerData as ServerHttpSensitivityGenerator.HeaderSensitivity.NotSensitiveMapValue).prefixHeader, null)

        val testProject = TestWorkspace.testProject(serverTestSymbolProvider(model))
        testProject.lib { writer ->
            writer.unitTest("header_closure") {
                withBlock("let closure = ", ";") {
                    generator.renderHeaderClosure(writer, headerData)
                }
                rustTemplate(
                    """
                    let name = #{Http}::header::HeaderName::from_static("header-a");
                    assert_eq!(closure(&name), #{SmithyHttpServer}::logging::sensitivity::headers::HeaderMarker { value: false, key_suffix: None });
                    let name = #{Http}::header::HeaderName::from_static("header-c");
                    assert_eq!(closure(&name), #{SmithyHttpServer}::logging::sensitivity::headers::HeaderMarker { value: true, key_suffix: None });
                    """,
                    *codegenScope,
                )
            }
        }
        testProject.compileAndTest()
    }

    @Test
    fun `prefix header closure`() {
        val model = """
            namespace test

            operation Secret {
                input: Input,
            }

            @sensitive
            structure Input {
                @required
                @httpPrefixHeaders("prefix-")
                prefixMap: PrefixMap,
            }

            map PrefixMap {
                key: String,
                value: String
            }

        """.asSmithyModel()
        val operation = model.operationShapes.toList()[0]
        val generator = ServerHttpSensitivityGenerator(model, operation, TestRuntimeConfig)

        val inputShape = operation.inputShape(model)
        val headerData = generator.findHeaderSensitivity(inputShape)
        assertEquals((headerData as ServerHttpSensitivityGenerator.HeaderSensitivity.SensitiveMapValue).prefixHeader, "prefix-")

        val testProject = TestWorkspace.testProject(serverTestSymbolProvider(model))
        testProject.lib { writer ->
            writer.unitTest("prefix_headers_closure") {
                withBlock("let closure = ", ";") {
                    generator.renderHeaderClosure(writer, headerData)
                }
                rustTemplate(
                    """
                    let name = #{Http}::header::HeaderName::from_static("prefix-a");
                    assert_eq!(closure(&name), #{SmithyHttpServer}::logging::sensitivity::headers::HeaderMarker { value: true, key_suffix: Some(7) });
                    let name = #{Http}::header::HeaderName::from_static("prefix-b");
                    assert_eq!(closure(&name), #{SmithyHttpServer}::logging::sensitivity::headers::HeaderMarker { value: true, key_suffix: Some(7) });
                    let name = #{Http}::header::HeaderName::from_static("other");
                    assert_eq!(closure(&name), #{SmithyHttpServer}::logging::sensitivity::headers::HeaderMarker { value: false, key_suffix: None });
                    """,
                    *codegenScope,
                )
            }
        }
        testProject.compileAndTest()
    }

    @Test
    fun `prefix headers key closure`() {
        val model = """
            namespace test

            operation Secret {
                input: Input,
            }

            structure Input {
                @required
                @httpPrefixHeaders("prefix-")
                prefix_map: PrefixMap,
            }

            @sensitive
            string SensitiveKey
            map PrefixMap {
                key: SensitiveKey,
                value: String
            }

        """.asSmithyModel()
        val operation = model.operationShapes.toList()[0]
        val generator = ServerHttpSensitivityGenerator(model, operation, TestRuntimeConfig)

        val inputShape = operation.inputShape(model)
        val headerData = generator.findHeaderSensitivity(inputShape)
        assert(headerData.headerKeys.isEmpty())
        val asMapValue = (headerData as ServerHttpSensitivityGenerator.HeaderSensitivity.NotSensitiveMapValue)
        assertEquals(asMapValue.prefixHeader, "prefix-")

        val testProject = TestWorkspace.testProject(serverTestSymbolProvider(model))
        testProject.lib { writer ->
            writer.unitTest("prefix_headers_special_closure") {
                withBlock("let closure = ", ";") {
                    generator.renderHeaderClosure(writer, headerData)
                }
                rustTemplate(
                    """
                    let name = #{Http}::header::HeaderName::from_static("prefix-a");
                    assert_eq!(closure(&name), #{SmithyHttpServer}::logging::sensitivity::headers::HeaderMarker { value: false, key_suffix: Some(7) });
                    let name = #{Http}::header::HeaderName::from_static("prefix-b");
                    assert_eq!(closure(&name), #{SmithyHttpServer}::logging::sensitivity::headers::HeaderMarker { value: false, key_suffix: Some(7) });
                    let name = #{Http}::header::HeaderName::from_static("other");
                    assert_eq!(closure(&name), #{SmithyHttpServer}::logging::sensitivity::headers::HeaderMarker { value: false, key_suffix: None });
                    """,
                    *codegenScope,
                )
            }
        }
        testProject.compileAndTest()
    }

    @Test
    fun `prefix headers value closure`() {
        val model = """
            namespace test

            operation Secret {
                input: Input,
            }

            structure Input {
                @required
                @httpPrefixHeaders("prefix-")
                prefix_map: PrefixMap,
            }

            @sensitive
            string SensitiveValue

            map PrefixMap {
                key: String,
                value: SensitiveValue
            }

        """.asSmithyModel()
        val operation = model.operationShapes.toList()[0]
        val generator = ServerHttpSensitivityGenerator(model, operation, TestRuntimeConfig)

        val inputShape = operation.inputShape(model)
        val headerData = generator.findHeaderSensitivity(inputShape)
        assert(headerData.headerKeys.isEmpty())
        val asSensitiveMapValue = (headerData as ServerHttpSensitivityGenerator.HeaderSensitivity.SensitiveMapValue)
        assertEquals(asSensitiveMapValue.prefixHeader, "prefix-")
        assertEquals(asSensitiveMapValue.keySensitive, false)

        val testProject = TestWorkspace.testProject(serverTestSymbolProvider(model))
        testProject.lib { writer ->
            writer.unitTest("prefix_headers_special_closure") {
                withBlock("let closure = ", ";") {
                    generator.renderHeaderClosure(writer, headerData)
                }
                rustTemplate(
                    """
                    let name = #{Http}::header::HeaderName::from_static("prefix-a");
                    assert_eq!(closure(&name), #{SmithyHttpServer}::logging::sensitivity::headers::HeaderMarker { value: true, key_suffix: None });
                    let name = #{Http}::header::HeaderName::from_static("prefix-b");
                    assert_eq!(closure(&name), #{SmithyHttpServer}::logging::sensitivity::headers::HeaderMarker { value: true, key_suffix: None });
                    let name = #{Http}::header::HeaderName::from_static("other");
                    assert_eq!(closure(&name), #{SmithyHttpServer}::logging::sensitivity::headers::HeaderMarker { value: false, key_suffix: None });
                    """,
                    *codegenScope,
                )
            }
        }
        testProject.compileAndTest()
    }

    @Test
    fun `uri closure`() {
        val model = """
            namespace test

            @http(method: "GET", uri: "/secret/{labelA}/{labelB}")
            operation Secret {
                input: Input,
            }

            @sensitive
            string SensitiveString

            structure Input {
                @required
                @httpLabel
                labelA: SensitiveString,
                @required
                @httpLabel
                labelB: SensitiveString,
            }
        """.asSmithyModel()
        val operation = model.operationShapes.toList()[0]
        val generator = ServerHttpSensitivityGenerator(model, operation, TestRuntimeConfig)

        val input = generator.input()!!
        val uri = operation.getTrait<HttpTrait>()!!.uri
        val labeledUriIndexes = generator.findUriLabelIndexes(uri, input)
        assertEquals(labeledUriIndexes, listOf(2, 1))

        val testProject = TestWorkspace.testProject(serverTestSymbolProvider(model))
        testProject.lib { writer ->
            writer.unitTest("uri_closure") {
                withBlock("let closure = ", ";") {
                    generator.renderLabelClosure(writer, labeledUriIndexes)
                }
                rustTemplate(
                    """
                    assert_eq!(closure(0), false);
                    assert_eq!(closure(1), true);
                    assert_eq!(closure(2), true);
                    """,
                    *codegenScope,
                )
            }
        }
        testProject.compileAndTest()
    }
}
