package software.amazon.smithy.rust.codegen.smithy.generators

import software.amazon.smithy.protocoltests.traits.HttpRequestTestCase
import software.amazon.smithy.protocoltests.traits.HttpRequestTestsTrait
import software.amazon.smithy.rust.codegen.lang.RustWriter
import software.amazon.smithy.rust.codegen.lang.rustBlock
import software.amazon.smithy.rust.codegen.lang.withBlock
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.util.dq

class HttpProtocolTestGenerator(private val protocolConfig: ProtocolConfig) {
    fun render() {
        with(protocolConfig) {
            operationShape.getTrait(HttpRequestTestsTrait::class.java).map {
                renderHttpRequestTests(it)
            }
        }
    }

    private fun renderHttpRequestTests(httpRequestTestsTrait: HttpRequestTestsTrait) {
        with(protocolConfig) {
            writer.write("#[cfg(test)]")
            val operationName = symbolProvider.toSymbol(operationShape).name
            val testModuleName = "${operationName.toSnakeCase()}_request_test"
            writer.withModule(testModuleName) {
                httpRequestTestsTrait.testCases.filter { it.protocol == protocol }.forEach { testCase ->
                    renderHttpRequestTestCase(testCase, this)
                }
            }
        }
    }

    private val instantiator = with(protocolConfig) {
        Instantiator(symbolProvider, model, runtimeConfig)
    }

    private fun renderHttpRequestTestCase(httpRequestTestCase: HttpRequestTestCase, testModuleWriter: RustWriter) {
        httpRequestTestCase.documentation.map {
            testModuleWriter.setNewlinePrefix("/// ").write(it).setNewlinePrefix("")
        }
        testModuleWriter.write("#[test]")
        testModuleWriter.rustBlock("fn test_${httpRequestTestCase.id.toSnakeCase()}()") {
            writeInline("let input =")
            instantiator.render(httpRequestTestCase.params, protocolConfig.inputShape, this)
            write(";")
            write("let http_request = input.build_http_request().body(()).unwrap();")
            with(httpRequestTestCase) {
                write(
                    """
                    assert_eq!(http_request.method(), ${method.dq()});
                    assert_eq!(http_request.uri().path(), ${uri.dq()});
                """
                )
                withBlock("let expected_query_params = vec![", "];") {
                    write(queryParams.joinToString(",") { it.dq() })
                }
                write(
                    "\$T(&http_request, expected_query_params.as_slice()).unwrap();",
                    RuntimeType.ProtocolTestHelper(protocolConfig.runtimeConfig, "validate_query_string")
                )
                // TODO: assert on the body contents
                write("/* BODY:\n ${body.orElse("[ No Body ]")} */")
            }
        }
    }
}
