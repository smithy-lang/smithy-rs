/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.transformers

import io.kotest.matchers.shouldBe
import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.ArgumentsSource
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.lookup
import software.amazon.smithy.rust.codegen.server.smithy.RecursiveConstraintViolationsTest
import software.amazon.smithy.rust.codegen.server.smithy.traits.ConstraintViolationRustBoxTrait
import kotlin.streams.toList

internal class RecursiveConstraintViolationBoxerTest {
    @ParameterizedTest
    @ArgumentsSource(RecursiveConstraintViolationsTest.RecursiveConstraintViolationsTestProvider::class)
    fun `recursive constraint violation boxer test`(testCase: RecursiveConstraintViolationsTest.TestCase) {
        val transformed = RecursiveConstraintViolationBoxer.transform(testCase.model)

        val shapesWithConstraintViolationRustBoxTrait = transformed.shapes().filter {
            it.hasTrait<ConstraintViolationRustBoxTrait>()
        }.toList()

        // Only the provided member shape should have the trait attached.
        shapesWithConstraintViolationRustBoxTrait shouldBe
            listOf(transformed.lookup(testCase.shapeIdWithConstraintViolationRustBoxTrait))
    }
}
