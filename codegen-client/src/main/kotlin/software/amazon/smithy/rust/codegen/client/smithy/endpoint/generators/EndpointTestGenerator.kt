/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.endpoint.generators

import software.amazon.smithy.rulesengine.language.evaluation.value.ArrayValue
import software.amazon.smithy.rulesengine.language.evaluation.value.BooleanValue
import software.amazon.smithy.rulesengine.language.evaluation.value.IntegerValue
import software.amazon.smithy.rulesengine.language.evaluation.value.RecordValue
import software.amazon.smithy.rulesengine.language.evaluation.value.StringValue
import software.amazon.smithy.rulesengine.language.evaluation.value.Value
import software.amazon.smithy.rulesengine.language.syntax.Identifier
import software.amazon.smithy.rulesengine.language.syntax.parameters.Parameters
import software.amazon.smithy.rulesengine.traits.EndpointTestCase
import software.amazon.smithy.rulesengine.traits.ExpectedEndpoint
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.Types
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.rustName
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
    codegenContext: ClientCodegenContext,
) {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val types = Types(runtimeConfig)
    private val codegenScope =
        arrayOf(
            "Endpoint" to types.smithyEndpoint,
            "Error" to types.resolveEndpointError,
            "Document" to RuntimeType.document(runtimeConfig),
            "HashMap" to RuntimeType.HashMap,
            "capture_request" to RuntimeType.captureRequest(runtimeConfig),
        )

    private fun EndpointTestCase.docs(): Writable {
        val self = this
        return writable { docs(self.documentation.orElse("no docs")) }
    }

    private fun generateBaseTest(
        testCase: EndpointTestCase,
        id: Int,
    ): Writable =
        writable {
            rustTemplate(
                """
                #{docs:W}
                ##[test]
                fn test_$id() {
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
                "assertion" to
                    writable {
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

    fun generate(): Writable =
        writable {
            var id = 0
            testCases.forEach { testCase ->
                id += 1
                generateBaseTest(testCase, id)(this)
            }
        }

    private fun params(testCase: EndpointTestCase) =
        writable {
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
                is StringValue -> rust(escape(value.value).dq() + ".to_string()")
                is BooleanValue -> rust(value.toString())
                is ArrayValue -> {
                    rust(
                        "vec![#W]",
                        value.values.map { member ->
                            writable {
                                rustTemplate(
                                    /*
                                     * If we wrote "#{Document}::from(#{value:W})" here, we could encounter a
                                     * compile error due to the following type mismatch:
                                     *  the trait `From<Vec<Document>>` is not implemented for `Vec<std::string::String>`
                                     *
                                     * given the following method signature:
                                     *  fn resource_arn_list(mut self, value: impl Into<::std::vec::Vec<::std::string::String>>)
                                     *
                                     * with a call site like this:
                                     *  .resource_arn_list(vec![::aws_smithy_types::Document::from(
                                     *      "arn:aws:dynamodb:us-east-1:333333333333:table/table_name".to_string(),
                                     *  )])
                                     *
                                     * For this reason we use `into()` instead to allow types that need to be converted
                                     * to `Document` to continue working as before, and to support the above use case.
                                     */
                                    "#{value:W}.into()",
                                    *codegenScope,
                                    "value" to generateValue(member),
                                )
                            }
                        }.join(","),
                    )
                }

                is IntegerValue -> rust(value.value.toString())

                is RecordValue ->
                    rustBlock("") {
                        rustTemplate(
                            "let mut out = #{HashMap}::<String, #{Document}>::new();",
                            *codegenScope,
                        )
                        val ids = mutableListOf<Identifier>()
                        value.value.forEach { (id, _) -> ids.add(id) }
                        ids.forEach { id ->
                            val v = value.get(id)
                            rust(
                                "out.insert(${id.toString().dq()}.to_string(), #W.into());",
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

    private fun generateEndpoint(value: ExpectedEndpoint) =
        writable {
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
