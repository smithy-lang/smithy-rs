/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.endpoint.generators

import software.amazon.smithy.rulesengine.language.eval.Value
import software.amazon.smithy.rulesengine.language.syntax.Identifier
import software.amazon.smithy.rulesengine.language.syntax.parameters.Parameters
import software.amazon.smithy.rulesengine.traits.EndpointTestCase
import software.amazon.smithy.rulesengine.traits.ExpectedEndpoint
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.EndpointCustomization
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.Types
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.rustName
import software.amazon.smithy.rust.codegen.client.smithy.generators.ClientInstantiator
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.docs
import software.amazon.smithy.rust.codegen.core.rustlang.escape
import software.amazon.smithy.rust.codegen.core.rustlang.join
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.util.PANIC
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.orNull

internal class EndpointTestGenerator(
    private val testCases: List<EndpointTestCase>,
    private val paramsType: RuntimeType,
    private val resolverType: RuntimeType,
    private val params: Parameters,
    private val endpointCustomizations: List<EndpointCustomization>,
    codegenContext: ClientCodegenContext,
) {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val serviceShape = codegenContext.serviceShape
    private val model = codegenContext.model
    private val types = Types(runtimeConfig)
    private val codegenScope = arrayOf(
        "Endpoint" to types.smithyEndpoint,
        "ResolveEndpoint" to types.resolveEndpoint,
        "Error" to types.resolveEndpointError,
        "Document" to RuntimeType.document(runtimeConfig),
        "HashMap" to RuntimeType.HashMap,
        "capture_request" to RuntimeType.captureRequest(runtimeConfig),
    )

    private val instantiator = ClientInstantiator(codegenContext)

    private fun EndpointTestCase.docs(): Writable {
        val self = this
        return writable { docs(self.documentation.orElse("no docs")) }
    }

    private fun generateBaseTest(testCase: EndpointTestCase, id: Int): Writable = writable {
        rustTemplate(
            """
            #{docs:W}
            ##[test]
            fn test_$id() {
                use #{ResolveEndpoint};
                let params = #{params:W};
                let resolver = #{resolver}::new();
                let endpoint = resolver.resolve_endpoint(&params);
                #{assertion:W}
            }
            """,
            *codegenScope,
            "docs" to testCase.docs(),
            "params" to params(testCase),
            "resolver" to resolverType,
            "assertion" to writable {
                testCase.expect.endpoint.ifPresent { endpoint ->
                    rustTemplate(
                        """
                        let endpoint = endpoint.expect("Expected valid endpoint: ${escape(endpoint.url)}");
                        assert_eq!(endpoint, #{expected:W});
                        """,
                        *codegenScope, "expected" to generateEndpoint(endpoint),
                    )
                }
                testCase.expect.error.ifPresent { error ->
                    val expectedError =
                        escape("expected error: $error [${testCase.documentation.orNull() ?: "no docs"}]")
                    rustTemplate(
                        """
                        let error = endpoint.expect_err(${expectedError.dq()});
                        assert_eq!(format!("{}", error), ${escape(error).dq()})
                        """,
                        *codegenScope,
                    )
                }
            },
        )
    }

    fun generate(): Writable = writable {
        var id = 0
        testCases.forEach { testCase ->
            id += 1
            generateBaseTest(testCase, id)(this)
        }
    }

    private fun params(testCase: EndpointTestCase) = writable {
        rust("#T::builder()", paramsType)
        testCase.params.members.forEach { (id, value) ->
            if (params.get(Identifier.of(id)).isPresent) {
                rust(".${Identifier.of(id).rustName()}(#W)", generateValue(Value.fromNode(value)))
            }
        }
        rust(""".build().expect("invalid params")""")
    }

    private fun generateValue(value: Value): Writable {
        return {
            when (value) {
                is Value.String -> rust(escape(value.value()).dq() + ".to_string()")
                is Value.Bool -> rust(value.toString())
                is Value.Array -> {
                    rust(
                        "vec![#W]",
                        value.values.map { member ->
                            writable {
                                rustTemplate(
                                    "#{Document}::from(#{value:W})",
                                    *codegenScope,
                                    "value" to generateValue(member),
                                )
                            }
                        }.join(","),
                    )
                }

                is Value.Integer -> rust(value.expectInteger().toString())

                is Value.Record ->
                    rustBlock("") {
                        rustTemplate(
                            "let mut out = #{HashMap}::<String, #{Document}>::new();",
                            *codegenScope,
                        )
                        // TODO(https://github.com/awslabs/smithy/pull/1555): remove sort by name when upgrading to
                        //   Smithy version with this PR merged
                        val keys = mutableListOf<Identifier>()
                        value.forEach { id, _ -> keys.add(id) }
                        keys.sortedBy { it.name.value }.forEach { identifier ->
                            val v = value.get(identifier)
                            rust(
                                "out.insert(${identifier.toString().dq()}.to_string(), #W.into());",
                                // When writing into the hashmap, it always needs to be an owned type
                                generateValue(v),
                            )
                        }
                        rustTemplate("out")
                    }

                else -> PANIC("unexpected type: $value")
            }
        }
    }

    private fun generateEndpoint(value: ExpectedEndpoint) = writable {
        rustTemplate("#{Endpoint}::builder().url(${escape(value.url).dq()})", *codegenScope)
        value.headers.forEach { (headerName, values) ->
            values.forEach { headerValue ->
                rust(".header(${headerName.dq()}, ${headerValue.dq()})")
            }
        }
        value.properties.forEach { (name, value) ->
            rust(
                ".property(${name.dq()}, #W)",
                generateValue(Value.fromNode(value)),
            )
        }
        rust(".build()")
    }
}
