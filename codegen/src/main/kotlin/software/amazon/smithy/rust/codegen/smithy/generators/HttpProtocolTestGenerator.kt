package software.amazon.smithy.rust.codegen.smithy.generators

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.protocoltests.traits.HttpRequestTestCase
import software.amazon.smithy.protocoltests.traits.HttpRequestTestsTrait
import software.amazon.smithy.rust.codegen.lang.RustWriter
import software.amazon.smithy.rust.codegen.lang.rustBlock
import software.amazon.smithy.rust.codegen.lang.withBlock
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.util.dq
import software.amazon.smithy.rust.codegen.util.inputShape

/**
 * Generate protocol tests for an operation
 */
class HttpProtocolTestGenerator(private val protocolConfig: ProtocolConfig, private val operationShape: OperationShape, private val writer: RustWriter) {
    val DenyList = setOf(
        "RestJsonListsSerializeNull",
        "AwsJson11MapsSerializeNullValues",
        "AwsJson11ListsSerializeNull"
    )
    private val inputShape = operationShape.inputShape(protocolConfig.model)
    fun render() {
        operationShape.getTrait(HttpRequestTestsTrait::class.java).map {
            renderHttpRequestTests(it)
        }
    }

    private fun renderHttpRequestTests(httpRequestTestsTrait: HttpRequestTestsTrait) {
        with(protocolConfig) {
            writer.write("#[cfg(test)]")
            val operationName = symbolProvider.toSymbol(operationShape).name
            val testModuleName = "${operationName.toSnakeCase()}_request_test"
            writer.withModule(testModuleName) {
                httpRequestTestsTrait.testCases.filter { it.protocol == protocol }.filter { !DenyList.contains(it.id) }.forEach { testCase ->
                    try {
                        renderHttpRequestTestCase(testCase, this)
                    } catch (ex: Exception) {
                        println("failed to generate ${testCase.id}")
                    }
                }
            }
        }
    }

    private val instantiator = with(protocolConfig) {
        Instantiator(symbolProvider, model, runtimeConfig)
    }

    private fun renderHttpRequestTestCase(httpRequestTestCase: HttpRequestTestCase, testModuleWriter: RustWriter) {
        testModuleWriter.setNewlinePrefix("/// ")
        httpRequestTestCase.documentation.map {
            testModuleWriter.write(it)
        }
        testModuleWriter.write("Test ID: ${httpRequestTestCase.id}")
        testModuleWriter.setNewlinePrefix("")
        testModuleWriter.write("#[test]")
        testModuleWriter.rustBlock("fn test_${httpRequestTestCase.id.toSnakeCase()}()") {
            writeInline("let input =")
            instantiator.render(httpRequestTestCase.params, inputShape, this)
            write(";")
            write("let http_request = input.build_http_request().body(()).unwrap();")
            with(httpRequestTestCase) {
                write(
                    """
                    assert_eq!(http_request.method(), ${method.dq()});
                    assert_eq!(http_request.uri().path(), ${uri.dq()});
                """
                )
            }
            checkQueryParams(this, httpRequestTestCase.queryParams)
            checkForbidQueryParams(this, httpRequestTestCase.forbidQueryParams)
            checkRequiredQueryParams(this, httpRequestTestCase.requireQueryParams)
            checkHeaders(this, httpRequestTestCase.headers)
            checkBody(this, httpRequestTestCase.body.orElse(""), httpRequestTestCase.bodyMediaType.orElse(null))
        }
    }

    private fun checkBody(rustWriter: RustWriter, body: String, mediaType: String?) {
        if (body == "") {
            rustWriter.write("// No body")
            rustWriter.write("assert!(input.build_body().is_empty());")
        } else {
            check(mediaType != null)
            // When we generate a body instead of a stub, drop the trailing `;` and enable the assertion
            // assertOk(rustWriter) {
            rustWriter.write(
                "let _ = \$T(input.build_body(), ${body.dq()}, \$T::from(${mediaType.dq()}));",
                RuntimeType.ProtocolTestHelper(protocolConfig.runtimeConfig, "validate_body"),
                RuntimeType.ProtocolTestHelper(protocolConfig.runtimeConfig, "MediaType")
            )
            // }
        }
    }

    private fun checkHeaders(rustWriter: RustWriter, headers: Map<String, String>) {
        if (headers.isEmpty()) {
            return
        }
        val variableName = "expected_headers"
        rustWriter.withBlock("let $variableName = &[", "];") {
            write(
                headers.entries.joinToString(",") {
                    "(${it.key.dq()}, ${it.value.dq()})"
                }
            )
        }
        assertOk(rustWriter) {
            write(
                "\$T(&http_request, $variableName)",
                RuntimeType.ProtocolTestHelper(protocolConfig.runtimeConfig, "validate_headers")
            )
        }
    }

    private fun checkRequiredQueryParams(
        rustWriter: RustWriter,
        requiredParams: List<String>
    ) = basicCheck(requiredParams, rustWriter, "required_params", "require_query_params")

    private fun checkForbidQueryParams(
        rustWriter: RustWriter,
        forbidParams: List<String>
    ) = basicCheck(forbidParams, rustWriter, "forbid_params", "forbid_query_params")

    private fun checkQueryParams(
        rustWriter: RustWriter,
        queryParams: List<String>
    ) = basicCheck(queryParams, rustWriter, "expected_query_params", "validate_query_string")

    private fun basicCheck(
        params: List<String>,
        rustWriter: RustWriter,
        variableName: String,
        checkFunction: String
    ) {
        if (params.isEmpty()) {
            return
        }
        rustWriter.withBlock("let $variableName = ", ";") {
            strSlice(this, params)
        }
        assertOk(rustWriter) {
            write(
                "\$T(&http_request, $variableName)",
                RuntimeType.ProtocolTestHelper(protocolConfig.runtimeConfig, checkFunction)
            )
        }
    }

    /**
     * wraps `inner` in a call to `protocol_test_helpers::assert_ok`, a convenience wrapper
     * for pretty prettying protocol test helper results
     */
    private fun assertOk(rustWriter: RustWriter, inner: RustWriter.() -> Unit) {
        rustWriter.write("\$T(", RuntimeType.ProtocolTestHelper(protocolConfig.runtimeConfig, "assert_ok"))
        inner(rustWriter)
        rustWriter.write(");")
    }

    private fun strSlice(writer: RustWriter, args: List<String>) {
        writer.withBlock("&[", "]") {
            write(args.joinToString(",") { it.dq() })
        }
    }
}
