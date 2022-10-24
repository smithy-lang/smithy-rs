package software.amazon.smithy.rust.codegen.client.smithy.endpoints

import software.amazon.smithy.rulesengine.language.EndpointRuleSet
import software.amazon.smithy.rulesengine.language.eval.Value
import software.amazon.smithy.rulesengine.language.syntax.Identifier
import software.amazon.smithy.rulesengine.traits.EndpointTestCase
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustMetadata
import software.amazon.smithy.rust.codegen.core.rustlang.asType
import software.amazon.smithy.rust.codegen.core.rustlang.docs
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.escapeTemplates
import software.amazon.smithy.rust.codegen.core.util.orNull

class EndpointTestsGenerator(
    private val codegenContext: ClientCodegenContext,
    private val endpointRuleset: EndpointRuleSet,
    private val testCases: List<EndpointTestCase>,
) {
    fun render(crate: RustCrate) {
        crate.withModule(EndpointsModule) { outerWriter ->
            outerWriter.withModule(
                "tests", RustMetadata(additionalAttributes = listOf(Attribute.Cfg("test"))),
            ) {
                testCases.withIndex().forEach { (index, testCase) ->
                    // Write docs if the test case included any
                    testCase.documentation.orNull()?.also { docs(it) }
                    docs("From: ${testCase.sourceLocation.filename}:${testCase.sourceLocation.line}")
                    rustTemplate(
                        """
                        ##[test]
                        fn test_$index() {
                            let params = #{params:W}.unwrap();
                            let endpoint = #{resolver}(&params);
                            #{assertion:W}
                        }
                    """,
                        "assertion" to assertion(testCase),
                        "params" to params(testCase),
                        "resolver" to EndpointsModule.member("inner_resolve_endpoint"),
                    )
                }
            }
        }
    }

    private fun assertion(testCase: EndpointTestCase) = writable {
        val expectationEndpoint = testCase.expect.endpoint.orNull()
        val expectationError = testCase.expect.error.orNull()
        if (expectationEndpoint != null) {
            val url = expectationEndpoint.url.dq().escapeTemplates()
            rustTemplate(
                """
                            let endpoint = endpoint.expect(r##"Expected URI: $url"##);
                            assert_eq!(#{Endpoint}::mutable($url.parse().unwrap()), endpoint);
                        """,
                "Endpoint" to CargoDependency.SmithyHttp(codegenContext.runtimeConfig).asType().member("endpoint::Endpoint"),
            )
        } else if (expectationError != null) {
            val err = expectationError.escapeTemplates()
            rust(
                """
                        let error = endpoint.expect_err(r##"expected error ${err.dq()}"##);
                        assert_eq!(r##"A valid endpoint could not be resolved: $err"##, error.to_string());
                    """,
            )
        }
    }

    private fun params(testCase: EndpointTestCase) = writable {
        rust("#T::default()", EndpointsModule.member("Builder"))
        testCase.params.members.forEach { (stringNode, valueNode) ->
            val id = Identifier.of(stringNode)
            if (endpointRuleset.parameters.get(Identifier.of(stringNode)).isPresent) {
                val value = Value.fromNode(valueNode)
                rust(".${id.toRustName()}(${generateValue(value)})")
            }
        }
        rust(".build()")
    }

    private fun generateValue(value: Value): String {
        return when (value) {
            is Value.String -> value.value().dq().escapeTemplates()
            is Value.Bool -> value.toString()
            else -> error("unexpected value")
        }
    }
}
