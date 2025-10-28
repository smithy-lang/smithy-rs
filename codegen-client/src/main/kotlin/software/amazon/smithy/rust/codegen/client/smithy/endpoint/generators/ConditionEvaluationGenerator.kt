/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.endpoint.generators

import software.amazon.smithy.rulesengine.language.syntax.expressions.functions.IsSet
import software.amazon.smithy.rulesengine.language.syntax.rule.Condition
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.Context
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.rulesgen.ExpressionGenerator
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.rulesgen.Ownership
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable

/**
 * Utility for generating condition evaluation code that returns a boolean.
 * Used by both rule-based and BDD-based endpoint resolvers.
 */
object ConditionEvaluationGenerator {
    /**
     * Generate code that evaluates a condition and returns a boolean result.
     *
     * For BDD conditions, we generate the expression directly and rely on Rust's type system.
     * The expression should evaluate to a boolean or Option<T> (which we convert to bool via is_some()).
     *
     * @param condition The condition to evaluate
     * @param context The code generation context
     * @param conditionIndex Optional index for BDD conditions to enable context-based result retrieval
     */
    fun generateConditionEvaluation(
        condition: Condition,
        context: Context,
        conditionIndex: Int? = null,
    ): Writable {
        val fn = condition.targetFunction()
        val generator = ExpressionGenerator(Ownership.Borrowed, context)
        val target = generator.generate(fn)

        return writable {
            // For IsSet conditions, check if the value is Some
            if (condition.function is IsSet) {
                rustTemplate("#{target:W}.is_some()", "target" to target)
            } else {
                // For other conditions, generate the expression directly
                // It should evaluate to a boolean
                rustTemplate("#{target:W}", "target" to target)
            }
        }
    }

    /**
     * Generate code that evaluates a condition and stores its result in context.
     * Returns a writable that evaluates to bool (true if result was Some, false otherwise).
     */
    fun generateConditionWithResultStorage(
        condition: Condition,
        context: Context,
        conditionIndex: Int,
    ): Writable {
        val fn = condition.targetFunction()
        val returnType = condition.function.functionDefinition.returnType
        val generator = ExpressionGenerator(Ownership.Borrowed, context)
        val target = generator.generate(fn)
        val resultName = condition.result.get().name

        return writable {
            rustTemplate(
                """
                {
                    let result = #{target:W};
                    if let Some(value) = result {
                        context.store(String::from("$resultName"), ConditionResult::String(value.to_string()));
                        true
                    } else {
                        false
                    }
                }
                """.trimIndent(),
                "target" to target,
            )
        }
    }

    /**
     * Deal with the actual target of the condition by flattening through isSet
     */
    private fun Condition.targetFunction() =
        when (val fn = this.function) {
            is IsSet -> fn.arguments[0]
            else -> fn
        }
}
