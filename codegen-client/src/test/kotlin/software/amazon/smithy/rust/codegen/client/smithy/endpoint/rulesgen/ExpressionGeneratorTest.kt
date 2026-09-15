/*
 *  Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 *  SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.endpoint.rulesgen

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.node.ArrayNode
import software.amazon.smithy.model.node.BooleanNode
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.rulesengine.language.syntax.Identifier
import software.amazon.smithy.rulesengine.language.syntax.expressions.Expression
import software.amazon.smithy.rulesengine.language.syntax.expressions.Template
import software.amazon.smithy.rulesengine.language.syntax.expressions.functions.BooleanEquals
import software.amazon.smithy.rulesengine.language.syntax.expressions.functions.StringEquals
import software.amazon.smithy.rulesengine.language.syntax.expressions.literal.Literal
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.Context
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.generators.FunctionRegistry
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.TestRuntimeConfig
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.core.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.core.testutil.unitTest

internal class ExprGeneratorTest {
    private val testContext = Context(FunctionRegistry(listOf()), TestRuntimeConfig)

    @Test
    fun generateExprs() {
        val boolEq = BooleanEquals.ofExpressions(Expression.of(true), Expression.of(true))
        val strEq = StringEquals.ofExpressions(Expression.of("helloworld"), Expression.of("goodbyeworld")).not()
        val combined = BooleanEquals.ofExpressions(boolEq, strEq)
        TestWorkspace.testProject().unitTest {
            val generator = ExpressionGenerator(Ownership.Borrowed, testContext)
            rust("assert_eq!(true, #W);", generator.generate(boolEq))
            rust("assert_eq!(true, #W);", generator.generate(strEq))
            rust("assert_eq!(true, #W);", generator.generate(combined))
        }.compileAndTest(runClippy = true)
    }

    @Test
    fun generateLiterals1() {
        val literal =
            Literal.recordLiteral(
                mutableMapOf(
                    Identifier.of("this") to Literal.integerLiteral(5),
                    Identifier.of("that") to
                        Literal.stringLiteral(
                            Template.fromString("static"),
                        ),
                ),
            )
        TestWorkspace.testProject().unitTest {
            val generator =
                ExpressionGenerator(Ownership.Borrowed, testContext)
            rust("""assert_eq!(Some(&(5.into())), #W.get("this"));""", generator.generate(literal))
        }.compileAndTest(runClippy = true)
    }

    @Test
    fun generateLiterals2() {
        val project = TestWorkspace.testProject()
        val gen =
            ExpressionGenerator(
                Ownership.Borrowed,
                Context(
                    FunctionRegistry(listOf()),
                    TestRuntimeConfig,
                ),
            )
        project.unitTest {
            rust("""let extra = "helloworld";""")
            rust("assert_eq!(true, #W);", gen.generate(Expression.of(true)))
            rust("assert_eq!(false, #W);", gen.generate(Expression.of(false)))
            rust("""assert_eq!("blah", #W);""", gen.generate(Expression.of("blah")))
            rust("""assert_eq!("helloworld: rust", #W);""", gen.generate(Expression.of("{extra}: rust")))
            rustTemplate(
                """
                let mut expected = std::collections::HashMap::new();
                expected.insert("a".to_string(), #{Document}::Bool(true));
                expected.insert("b".to_string(), #{Document}::String("hello".to_string()));
                expected.insert("c".to_string(), #{Document}::Array(vec![true.into()]));
                assert_eq!(expected, #{actual:W});
                """,
                "Document" to RuntimeType.document(TestRuntimeConfig),
                "actual" to
                    gen.generate(
                        Literal.fromNode(
                            Node.objectNode().withMember("a", true).withMember("b", "hello")
                                .withMember("c", ArrayNode.arrayNode(BooleanNode.from(true))),
                        ),
                    ),
            )
        }.compileAndTest(runClippy = true)
    }
}
