/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Test
import software.amazon.smithy.rustsdk.AwsReadmeDecorator

class AwsReadmeDecoratorTest {
    @Test
    fun `it converts description HTML into Markdown`() {
        assertEquals(
            """
            This is __some paragraph__ of information.

            This is _another_ paragraph of information.

            More information [can be found here](https://example.com).
            """.trimIndent(),
            AwsReadmeDecorator().normalizeDescription(
                "",
                """
                <fullname>Some service</fullname>
                <p>This is <b>some paragraph</b>
                of information.</p>
                <p>This is <i>another</i> paragraph
                of information.</p>
                <p>More information <a href="https://example.com">can be found here</a>.</p>
                """.trimIndent()
            )
        )
    }

    @Test
    fun `it converts lists`() {
        assertEquals(
            """
            Some text introducing a list:

              - foo bar baz
              - baz bar foo
                1. nested item
                1. another

            More text.
            """.trimIndent(),
            AwsReadmeDecorator().normalizeDescription(
                "",
                """
                <p>Some text introducing a list:
                <ul>
                  <li>foo bar baz</li>
                  <li>baz bar foo
                    <ol><li>nested item</li><li>another</li></ol>
                  </li>
                </ul> More text.</p>
                """.trimIndent()
            )
        )
    }
}
