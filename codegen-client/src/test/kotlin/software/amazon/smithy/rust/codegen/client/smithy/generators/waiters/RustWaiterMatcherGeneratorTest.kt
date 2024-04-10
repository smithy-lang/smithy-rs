/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.client.smithy.generators.waiters

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.lookup
import software.amazon.smithy.rust.codegen.core.util.outputShape
import software.amazon.smithy.waiters.Matcher.SuccessMember

private typealias Scope = Array<Pair<String, Any>>

class RustWaiterMatcherGeneratorTest {
    class TestCase(
        codegenContext: ClientCodegenContext,
        private val rustCrate: RustCrate,
        matcherJson: String,
    ) {
        val operationShape = codegenContext.model.lookup<OperationShape>("test#TestOperation")
        val inputShape = operationShape.inputShape(codegenContext.model)
        val outputShape = operationShape.outputShape(codegenContext.model)
        val errorShape = codegenContext.model.lookup<StructureShape>("test#SomeError")
        val inputSymbol = codegenContext.symbolProvider.toSymbol(inputShape)
        val outputSymbol = codegenContext.symbolProvider.toSymbol(outputShape)
        val errorSymbol = codegenContext.symbolProvider.toSymbol(errorShape)

        val matcher = SuccessMember.fromNode(Node.parse(matcherJson))
        val matcherFn =
            RustWaiterMatcherGenerator(codegenContext, "TestOperation", inputShape, outputShape)
                .generate(errorSymbol, matcher)

        val scope =
            arrayOf(
                *preludeScope,
                "Input" to inputSymbol,
                "Output" to outputSymbol,
                "Error" to errorSymbol,
                "ErrorMetadata" to RuntimeType.errorMetadata(codegenContext.runtimeConfig),
                "matcher_fn" to matcherFn,
            )

        fun renderTest(
            name: String,
            writeTest: TestCase.() -> Writable,
        ) {
            rustCrate.lib {
                rustTemplate(
                    """
                    /// Make the unit test public and document it so that compiler
                    /// doesn't complain about dead code.
                    pub fn ${name}_test_case() {
                        #{test}
                    }
                    ##[cfg(test)]
                    ##[test]
                    fn $name() {
                        ${name}_test_case();
                    }
                    """,
                    *scope,
                    "test" to writeTest(),
                )
            }
        }
    }

    @Test
    fun tests() {
        clientIntegrationTest(testModel()) { codegenContext, rustCrate ->
            successMatcher(codegenContext, rustCrate)
            errorMatcher(codegenContext, rustCrate)
            outputPathMatcher(codegenContext, rustCrate)
            inputOutputPathMatcher(codegenContext, rustCrate)
        }
    }

    private fun testCase(
        codegenContext: ClientCodegenContext,
        rustCrate: RustCrate,
        name: String,
        matcherJson: String,
        writeFn: RustWriter.(Scope) -> Unit,
    ) {
        TestCase(codegenContext, rustCrate, matcherJson).renderTest(name) {
            writable {
                writeFn(scope)
            }
        }
    }

    private fun successMatcher(
        codegenContext: ClientCodegenContext,
        rustCrate: RustCrate,
    ) = testCase(
        codegenContext,
        rustCrate,
        name = "success_matcher",
        matcherJson = """{"success":true}""",
    ) { scope ->
        rustTemplate(
            """
            let input = #{Input}::builder().foo("foo").build().unwrap();
            let result = #{Ok}(#{Output}::builder().some_string("bar").build());
            assert!(#{matcher_fn}(&input, &result));

            let result = #{Err}(#{Error}::builder().message("asdf").build());
            assert!(!#{matcher_fn}(&input, &result));
            """,
            *scope,
        )
    }

    private fun errorMatcher(
        codegenContext: ClientCodegenContext,
        rustCrate: RustCrate,
    ) = testCase(
        codegenContext,
        rustCrate,
        name = "error_matcher",
        matcherJson = """{"errorType":"SomeError"}""",
    ) { scope ->
        rustTemplate(
            """
            let input = #{Input}::builder().foo("foo").build().unwrap();
            let result = #{Ok}(#{Output}::builder().some_string("bar").build());
            assert!(!#{matcher_fn}(&input, &result));

            let result = #{Err}(
                #{Error}::builder()
                    .message("asdf")
                    .meta(#{ErrorMetadata}::builder().code("SomeOtherError").build())
                    .build()
            );
            assert!(!#{matcher_fn}(&input, &result));

            let result = #{Err}(
                #{Error}::builder()
                    .message("asdf")
                    .meta(#{ErrorMetadata}::builder().code("SomeError").build())
                    .build()
            );
            assert!(#{matcher_fn}(&input, &result));
            """,
            *scope,
        )
    }

    private fun outputPathMatcher(
        codegenContext: ClientCodegenContext,
        rustCrate: RustCrate,
    ) {
        fun test(
            name: String,
            matcherJson: String,
            writeFn: RustWriter.(Scope) -> Unit,
        ) = testCase(codegenContext, rustCrate, name, matcherJson, writeFn)

        fun matcherJson(
            path: String,
            expected: String,
            comparator: String,
        ) = """{"output":{"path":${path.dq()}, "expected":${expected.dq()}, "comparator": ${comparator.dq()}}}"""

        test(
            "output_path_matcher_string_equals",
            matcherJson(
                path = "someString",
                expected = "expected-value",
                comparator = "stringEquals",
            ),
        ) { scope ->
            rustTemplate(
                """
                let input = #{Input}::builder().foo("foo").build().unwrap();
                let result = #{Ok}(#{Output}::builder().some_string("bar").build());
                assert!(!#{matcher_fn}(&input, &result));

                let result = #{Ok}(#{Output}::builder().some_string("expected-value").build());
                assert!(#{matcher_fn}(&input, &result));
                """,
                *scope,
            )
        }

        test(
            "output_path_matcher_bool_equals",
            matcherJson(
                path = "someBool",
                expected = "true",
                comparator = "booleanEquals",
            ),
        ) { scope ->
            rustTemplate(
                """
                let input = #{Input}::builder().foo("foo").build().unwrap();
                let result = #{Ok}(#{Output}::builder().some_bool(false).build());
                assert!(!#{matcher_fn}(&input, &result));

                let result = #{Ok}(#{Output}::builder().some_bool(true).build());
                assert!(#{matcher_fn}(&input, &result));
                """,
                *scope,
            )
        }

        test(
            "output_path_matcher_all_string_equals",
            matcherJson(
                path = "someList",
                expected = "foo",
                comparator = "allStringEquals",
            ),
        ) { scope ->
            rustTemplate(
                """
                let input = #{Input}::builder().foo("foo").build().unwrap();
                let result = #{Ok}(#{Output}::builder()
                    .some_list("foo")
                    .some_list("bar")
                    .build());
                assert!(!#{matcher_fn}(&input, &result));

                let result = #{Ok}(#{Output}::builder()
                    .some_list("foo")
                    .some_list("foo")
                    .build());
                assert!(#{matcher_fn}(&input, &result));
                """,
                *scope,
            )
        }

        test(
            "output_path_matcher_any_string_equals",
            matcherJson(
                path = "someList",
                expected = "foo",
                comparator = "anyStringEquals",
            ),
        ) { scope ->
            rustTemplate(
                """
                let input = #{Input}::builder().foo("foo").build().unwrap();
                let result = #{Ok}(#{Output}::builder()
                    .some_list("bar")
                    .build());
                assert!(!#{matcher_fn}(&input, &result));

                let result = #{Ok}(#{Output}::builder()
                    .some_list("bar")
                    .some_list("foo")
                    .build());
                assert!(#{matcher_fn}(&input, &result));
                """,
                *scope,
            )
        }
    }

    private fun inputOutputPathMatcher(
        codegenContext: ClientCodegenContext,
        rustCrate: RustCrate,
    ) {
        fun test(
            name: String,
            matcherJson: String,
            writeFn: RustWriter.(Scope) -> Unit,
        ) = testCase(codegenContext, rustCrate, name, matcherJson, writeFn)

        fun matcherJson(
            path: String,
            expected: String,
            comparator: String,
        ) = """{"inputOutput":{"path":${path.dq()}, "expected":${expected.dq()}, "comparator": ${comparator.dq()}}}"""

        test(
            "input_output_path_matcher_boolean_equals",
            matcherJson(
                path = "input.foo == 'foo' && output.someString == 'bar'",
                expected = "true",
                comparator = "booleanEquals",
            ),
        ) { scope ->
            rustTemplate(
                """
                let input = #{Input}::builder().foo("foo").build().unwrap();
                let result = #{Ok}(#{Output}::builder().some_string("bar").build());
                assert!(#{matcher_fn}(&input, &result));

                let input = #{Input}::builder().foo("asdf").build().unwrap();
                assert!(!#{matcher_fn}(&input, &result));
                """,
                *scope,
            )
        }
    }

    private fun testModel() =
        """
        ${'$'}version: "2"
        namespace test

        @aws.protocols#awsJson1_0
        service TestService {
            operations: [TestOperation],
        }

        operation TestOperation {
            input: GetEntityRequest,
            output: GetEntityResponse,
            errors: [SomeError],
        }

        @error("server")
        structure SomeError {
            message: String,
        }

        structure GetEntityRequest {
            foo: String,
        }

        structure GetEntityResponse {
            someString: String,
            someBool: Boolean,
            someList: SomeList,
        }

        list SomeList {
            member: String
        }
        """.asSmithyModel()
}
