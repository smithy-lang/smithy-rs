/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.traits.HttpTrait
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.testutil.ServerTestRuntimeConfig
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverTestSymbolProvider

class ServerHttpSensitivityGeneratorTest {
    private val codegenScope =
        arrayOf(
            "SmithyHttpServer" to ServerCargoDependency.smithyHttpServer(ServerTestRuntimeConfig).toType(),
            "Http" to CargoDependency.Http0x.toType(),
        )

    @Test
    fun `query closure`() {
        val model =
            """
            namespace test

            operation Secret {
                input: Input,
            }

            @sensitive
            string SensitiveString

            structure Input {
                @required
                @httpQuery("query_a")
                queryA: String,

                @required
                @httpQuery("query_b")
                queryB: SensitiveString
            }
            """.asSmithyModel()
        val operation = model.operationShapes.toList()[0]
        val generator = ServerHttpSensitivityGenerator(model, operation, ServerTestRuntimeConfig)

        val input = generator.input()!!
        val querySensitivity = generator.findQuerySensitivity(input)
        assert(!querySensitivity.allKeysSensitive)
        assertEquals(listOf("query_b"), (querySensitivity as QuerySensitivity.NotSensitiveMapValue).queryKeys)

        val testProject = TestWorkspace.testProject(serverTestSymbolProvider(model))
        testProject.testModule {
            unitTest("query_closure") {
                rustTemplate(
                    """
                    let closure = #{Closure:W};
                    assert_eq!(closure("query_a"), #{SmithyHttpServer}::instrumentation::sensitivity::uri::QueryMarker { key: false, value: false });
                    assert_eq!(closure("query_b"), #{SmithyHttpServer}::instrumentation::sensitivity::uri::QueryMarker { key: false, value: true });
                    """,
                    "Closure" to querySensitivity.closure(),
                    *codegenScope,
                )
            }
        }

        testProject.compileAndTest()
    }

    @Test
    fun `query params closure`() {
        val model =
            """
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
        val generator = ServerHttpSensitivityGenerator(model, operation, ServerTestRuntimeConfig)

        val input = generator.input()!!
        val querySensitivity = generator.findQuerySensitivity(input)

        assert(querySensitivity.allKeysSensitive)
        querySensitivity as QuerySensitivity.SensitiveMapValue

        val testProject = TestWorkspace.testProject(serverTestSymbolProvider(model))
        testProject.testModule {
            unitTest("query_params_closure") {
                rustTemplate(
                    """
                    let closure = #{Closure:W};
                    assert_eq!(closure("wildcard"), #{SmithyHttpServer}::instrumentation::sensitivity::uri::QueryMarker { key: true, value: true });
                    """,
                    "Closure" to querySensitivity.closure(),
                    *codegenScope,
                )
            }
        }
        testProject.compileAndTest()
    }

    @Test
    fun `query params key closure`() {
        val model =
            """
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
        val generator = ServerHttpSensitivityGenerator(model, operation, ServerTestRuntimeConfig)

        val input = generator.input()!!
        val querySensitivity = generator.findQuerySensitivity(input)
        assert(querySensitivity.allKeysSensitive)
        assert((querySensitivity as QuerySensitivity.NotSensitiveMapValue).queryKeys.isEmpty())

        val testProject = TestWorkspace.testProject(serverTestSymbolProvider(model))
        testProject.testModule {
            unitTest("query_params_special_closure") {
                rustTemplate(
                    """
                    let closure = #{Closure:W};
                    assert_eq!(closure("wildcard"), #{SmithyHttpServer}::instrumentation::sensitivity::uri::QueryMarker { key: true, value: false });
                    """,
                    "Closure" to querySensitivity.closure(),
                    *codegenScope,
                )
            }
        }
        testProject.compileAndTest()
    }

    @Test
    fun `query params value closure`() {
        val model =
            """
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
        val generator = ServerHttpSensitivityGenerator(model, operation, ServerTestRuntimeConfig)

        val input = generator.input()!!
        val querySensitivity = generator.findQuerySensitivity(input)
        assert(!querySensitivity.allKeysSensitive)
        querySensitivity as QuerySensitivity.SensitiveMapValue

        val testProject = TestWorkspace.testProject(serverTestSymbolProvider(model))
        testProject.testModule {
            unitTest("query_params_special_closure") {
                rustTemplate(
                    """
                    let closure = #{Closure:W};
                    assert_eq!(closure("wildcard"), #{SmithyHttpServer}::instrumentation::sensitivity::uri::QueryMarker { key: false, value: true });
                    """,
                    "Closure" to querySensitivity.closure(),
                    *codegenScope,
                )
            }
        }
        testProject.compileAndTest()
    }

    @Test
    fun `query params none`() {
        val model =
            """
            namespace test

            operation Secret {
                input: Input,
            }

            structure Input {
                @required
                @httpQueryParams()
                queryMap: QueryMap,
            }

            map QueryMap {
                key: String,
                value: String
            }

            """.asSmithyModel()
        val operation = model.operationShapes.toList()[0]
        val generator = ServerHttpSensitivityGenerator(model, operation, ServerTestRuntimeConfig)

        val input = generator.input()!!
        val querySensitivity = generator.findQuerySensitivity(input)
        assert(!querySensitivity.allKeysSensitive)
        querySensitivity as QuerySensitivity.NotSensitiveMapValue
        assert(!querySensitivity.hasRedactions())
    }

    @Test
    fun `header closure`() {
        val model =
            """
            namespace test

            operation Secret {
                input: Input,
            }

            @sensitive
            string SensitiveString

            structure Input {
                @required
                @httpHeader("header-a")
                headerA: String,

                @required
                @httpHeader("header-b")
                headerB: SensitiveString
            }
            """.asSmithyModel()
        val operation = model.operationShapes.toList()[0]
        val generator = ServerHttpSensitivityGenerator(model, operation, ServerTestRuntimeConfig)

        val inputShape = operation.inputShape(model)
        val headerData = generator.findHeaderSensitivity(inputShape)
        assertEquals(listOf("header-b"), headerData.headerKeys)
        assertEquals(null, (headerData as HeaderSensitivity.NotSensitiveMapValue).prefixHeader)

        val testProject = TestWorkspace.testProject(serverTestSymbolProvider(model))
        testProject.testModule {
            unitTest("header_closure") {
                rustTemplate(
                    """
                    let closure = #{Closure:W};
                    let name = #{Http}::header::HeaderName::from_static("header-a");
                    assert_eq!(closure(&name), #{SmithyHttpServer}::instrumentation::sensitivity::headers::HeaderMarker { value: false, key_suffix: None });
                    let name = #{Http}::header::HeaderName::from_static("header-b");
                    assert_eq!(closure(&name), #{SmithyHttpServer}::instrumentation::sensitivity::headers::HeaderMarker { value: true, key_suffix: None });
                    """,
                    "Closure" to headerData.closure(),
                    *codegenScope,
                )
            }
        }
        testProject.compileAndTest()
    }

    @Test
    fun `prefix header closure`() {
        val model =
            """
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
        val generator = ServerHttpSensitivityGenerator(model, operation, ServerTestRuntimeConfig)

        val inputShape = operation.inputShape(model)
        val headerData = generator.findHeaderSensitivity(inputShape)
        assertEquals("prefix-", (headerData as HeaderSensitivity.SensitiveMapValue).prefixHeader)

        val testProject = TestWorkspace.testProject(serverTestSymbolProvider(model))
        testProject.testModule {
            unitTest("prefix_headers_closure") {
                rustTemplate(
                    """
                    let closure = #{Closure:W};
                    let name = #{Http}::header::HeaderName::from_static("prefix-a");
                    assert_eq!(closure(&name), #{SmithyHttpServer}::instrumentation::sensitivity::headers::HeaderMarker { value: true, key_suffix: Some(7) });
                    let name = #{Http}::header::HeaderName::from_static("prefix-b");
                    assert_eq!(closure(&name), #{SmithyHttpServer}::instrumentation::sensitivity::headers::HeaderMarker { value: true, key_suffix: Some(7) });
                    let name = #{Http}::header::HeaderName::from_static("other");
                    assert_eq!(closure(&name), #{SmithyHttpServer}::instrumentation::sensitivity::headers::HeaderMarker { value: false, key_suffix: None });
                    """,
                    "Closure" to headerData.closure(),
                    *codegenScope,
                )
            }
        }
        testProject.compileAndTest()
    }

    @Test
    fun `prefix header none`() {
        val model =
            """
            namespace test

            operation Secret {
                input: Input,
            }

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
        val generator = ServerHttpSensitivityGenerator(model, operation, ServerTestRuntimeConfig)

        val inputShape = operation.inputShape(model)
        val headerData = generator.findHeaderSensitivity(inputShape)
        headerData as HeaderSensitivity.NotSensitiveMapValue
        assert(!headerData.hasRedactions())
    }

    @Test
    fun `prefix headers key closure`() {
        val model =
            """
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
        val generator = ServerHttpSensitivityGenerator(model, operation, ServerTestRuntimeConfig)

        val inputShape = operation.inputShape(model)
        val headerData = generator.findHeaderSensitivity(inputShape)
        assert(headerData.headerKeys.isEmpty())
        val asMapValue = (headerData as HeaderSensitivity.NotSensitiveMapValue)
        assertEquals("prefix-", asMapValue.prefixHeader)

        val testProject = TestWorkspace.testProject(serverTestSymbolProvider(model))
        testProject.testModule {
            unitTest("prefix_headers_special_closure") {
                rustTemplate(
                    """
                    let closure = #{Closure:W};
                    let name = #{Http}::header::HeaderName::from_static("prefix-a");
                    assert_eq!(closure(&name), #{SmithyHttpServer}::instrumentation::sensitivity::headers::HeaderMarker { value: false, key_suffix: Some(7) });
                    let name = #{Http}::header::HeaderName::from_static("prefix-b");
                    assert_eq!(closure(&name), #{SmithyHttpServer}::instrumentation::sensitivity::headers::HeaderMarker { value: false, key_suffix: Some(7) });
                    let name = #{Http}::header::HeaderName::from_static("other");
                    assert_eq!(closure(&name), #{SmithyHttpServer}::instrumentation::sensitivity::headers::HeaderMarker { value: false, key_suffix: None });
                    """,
                    "Closure" to headerData.closure(),
                    *codegenScope,
                )
            }
        }
        testProject.compileAndTest()
    }

    @Test
    fun `prefix headers value closure`() {
        val model =
            """
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
        val generator = ServerHttpSensitivityGenerator(model, operation, ServerTestRuntimeConfig)

        val inputShape = operation.inputShape(model)
        val headerData = generator.findHeaderSensitivity(inputShape)
        assert(headerData.headerKeys.isEmpty())
        val asSensitiveMapValue = (headerData as HeaderSensitivity.SensitiveMapValue)
        assertEquals("prefix-", asSensitiveMapValue.prefixHeader)
        assert(!asSensitiveMapValue.keySensitive)

        val testProject = TestWorkspace.testProject(serverTestSymbolProvider(model))
        testProject.testModule {
            unitTest("prefix_headers_special_closure") {
                rustTemplate(
                    """
                    let closure = #{Closure:W};
                    let name = #{Http}::header::HeaderName::from_static("prefix-a");
                    assert_eq!(closure(&name), #{SmithyHttpServer}::instrumentation::sensitivity::headers::HeaderMarker { value: true, key_suffix: None });
                    let name = #{Http}::header::HeaderName::from_static("prefix-b");
                    assert_eq!(closure(&name), #{SmithyHttpServer}::instrumentation::sensitivity::headers::HeaderMarker { value: true, key_suffix: None });
                    let name = #{Http}::header::HeaderName::from_static("other");
                    assert_eq!(closure(&name), #{SmithyHttpServer}::instrumentation::sensitivity::headers::HeaderMarker { value: false, key_suffix: None });
                    """,
                    "Closure" to headerData.closure(),
                    *codegenScope,
                )
            }
        }
        testProject.compileAndTest()
    }

    @Test
    fun `uri closure`() {
        val model =
            """
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
        val generator = ServerHttpSensitivityGenerator(model, operation, ServerTestRuntimeConfig)

        val input = generator.input()!!
        val uri = operation.getTrait<HttpTrait>()!!.uri
        val labelData = generator.findLabelSensitivity(uri, input)

        assertEquals(listOf(1, 2), labelData.labelIndexes)

        val testProject = TestWorkspace.testProject(serverTestSymbolProvider(model))
        testProject.testModule {
            unitTest("uri_closure") {
                rustTemplate(
                    """
                    let closure = #{Closure:W};
                    assert_eq!(closure(0), false);
                    assert_eq!(closure(1), true);
                    assert_eq!(closure(2), true);
                    """,
                    "Closure" to labelData.closure(),
                    *codegenScope,
                )
            }
        }
        testProject.compileAndTest()
    }

    @Test
    fun `uri greedy`() {
        val model =
            """
            namespace test

            @http(method: "GET", uri: "/secret/{labelA}/{labelB+}/labelC")
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
        val generator = ServerHttpSensitivityGenerator(model, operation, ServerTestRuntimeConfig)

        val input = generator.input()!!
        val uri = operation.getTrait<HttpTrait>()!!.uri
        val labelData = generator.findLabelSensitivity(uri, input)

        assertEquals(listOf(1), labelData.labelIndexes)

        val greedyLabel = labelData.greedyLabel!!
        assertEquals(greedyLabel.segmentIndex, 2)
        assertEquals(greedyLabel.endOffset, 7)
    }
}
