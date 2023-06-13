/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import io.kotest.assertions.throwables.shouldThrow
import io.kotest.matchers.collections.shouldHaveSize
import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.validation.Severity
import software.amazon.smithy.model.validation.ValidatedResultException
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel

class PatternTraitEscapedSpecialCharsValidatorTest {
    @Test
    fun `should error out with a suggestion if non-escaped special chars used inside @pattern`() {
        val exception = shouldThrow<ValidatedResultException> {
            """
            namespace test
    
            @pattern("\t")
            string MyString
            """.asSmithyModel(smithyVersion = "2")
        }
        val events = exception.validationEvents.filter { it.severity == Severity.ERROR }

        events shouldHaveSize 1
        events[0].shapeId.get() shouldBe ShapeId.from("test#MyString")
        events[0].message shouldBe """
            Non-escaped special characters used inside `@pattern`.
            You must escape them: `@pattern("\\t")`.
            See https://github.com/awslabs/smithy-rs/issues/2508 for more details.
        """.trimIndent()
    }

    @Test
    fun `should suggest escaping spacial characters properly`() {
        val exception = shouldThrow<ValidatedResultException> {
            """
            namespace test
    
            @pattern("[.\n\\r]+")
            string MyString
            """.asSmithyModel(smithyVersion = "2")
        }
        val events = exception.validationEvents.filter { it.severity == Severity.ERROR }

        events shouldHaveSize 1
        events[0].shapeId.get() shouldBe ShapeId.from("test#MyString")
        events[0].message shouldBe """
            Non-escaped special characters used inside `@pattern`.
            You must escape them: `@pattern("[.\\n\\r]+")`.
            See https://github.com/awslabs/smithy-rs/issues/2508 for more details.
        """.trimIndent()
    }

    @Test
    fun `should report all non-escaped special characters`() {
        val exception = shouldThrow<ValidatedResultException> {
            """
            namespace test
    
            @pattern("\b")
            string MyString
            
            @pattern("^\n$")
            string MyString2
            
            @pattern("^[\n]+$")
            string MyString3
            
            @pattern("^[\r\t]$")
            string MyString4
            """.asSmithyModel(smithyVersion = "2")
        }
        val events = exception.validationEvents.filter { it.severity == Severity.ERROR }
        events shouldHaveSize 4
    }

    @Test
    fun `should report errors on string members`() {
        val exception = shouldThrow<ValidatedResultException> {
            """
            namespace test
    
            @pattern("\t")
            string MyString
            
            structure MyStructure {
                @pattern("\b")
                field: String
            }
            """.asSmithyModel(smithyVersion = "2")
        }
        val events = exception.validationEvents.filter { it.severity == Severity.ERROR }

        events shouldHaveSize 2
        events[0].shapeId.get() shouldBe ShapeId.from("test#MyString")
        events[1].shapeId.get() shouldBe ShapeId.from("test#MyStructure\$field")
    }

    @Test
    fun `shouldn't error out if special chars are properly escaped`() {
        """
        namespace test

        @pattern("\\t")
        string MyString
        
        @pattern("[.\\n\\r]+")
        string MyString2
        
        @pattern("\\b\\f\\n\\r\\t")
        string MyString3

        @pattern("\\w+")
        string MyString4
        """.asSmithyModel(smithyVersion = "2")
    }
}
