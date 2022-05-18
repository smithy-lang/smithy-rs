/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.util

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test

internal class StringsTest {

    @Test
    fun doubleQuote() {
        "abc".doubleQuote() shouldBe "\"abc\""
        """{"some": "json"}""".doubleQuote() shouldBe """"{\"some\": \"json\"}""""
        """{"nested": "{\"nested\": 5}"}"}""".doubleQuote() shouldBe """
        "{\"nested\": \"{\\\"nested\\\": 5}\"}\"}"
        """.trimIndent().trim()
    }
}
