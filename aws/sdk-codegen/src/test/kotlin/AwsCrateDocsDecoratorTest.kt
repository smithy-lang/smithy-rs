/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.loader.ModelAssembler
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.client.testutil.testClientCodegenContext
import software.amazon.smithy.rust.codegen.client.testutil.testClientRustSettings
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rustsdk.AwsCrateDocGenerator
import software.amazon.smithy.rustsdk.AwsTestRuntimeConfig

class AwsCrateDocsDecoratorTest {
    private val codegenContext = testClientCodegenContext(ModelAssembler().assemble().unwrap())

    @Test
    fun `it converts description HTML into Markdown`() {
        assertEquals(
            """
            This is __some paragraph__ of information.

            This is _another_ paragraph of information.

            More information [can be found here](https://example.com).
            """.trimIndent(),
            AwsCrateDocGenerator(codegenContext).normalizeDescription(
                "",
                """
                <fullname>Some service</fullname>
                <p>This is <b>some paragraph</b>
                of information.</p>
                <p>This is <i>another</i> paragraph
                of information.</p>
                <p>More information <a href="https://example.com">can be found here</a>.</p>
                """.trimIndent(),
            ),
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
            AwsCrateDocGenerator(codegenContext).normalizeDescription(
                "",
                """
                <p>Some text introducing a list:
                <ul>
                  <li>foo bar baz</li>
                  <li>baz bar foo
                    <ol><li>nested item</li><li>another</li></ol>
                  </li>
                </ul> More text.</p>
                """.trimIndent(),
            ),
        )
    }

    @Test
    fun `it converts description lists`() {
        assertEquals(
            """
            Some text introducing a description list:

            __Something__

            Some description of [something](test).

            __Another thing__

            Some description of another thing.

            A second paragraph that describes another thing.

            __MDN says these can be wrapped in divs__

            So here we are

            Some trailing text.
            """.trimIndent(),
            AwsCrateDocGenerator(codegenContext).normalizeDescription(
                "",
                """
                <p>Some text introducing a description list:
                <dl>
                  <dt>Something</dt>
                  <dd>Some description of <a href="test">something</a>.</dd>
                  <dt>Another thing</dt>
                  <dd>Some description of another thing.</dd>
                  <dd>A second paragraph that describes another thing.</dd>
                  <div>
                      <dt>MDN says these can be wrapped in divs</dt>
                      <dd>So here we are</dd>
                  </div>
                </dl>
                Some trailing text.
                </p>
                """.trimIndent(),
            ),
        )
    }

    // TODO(PostGA): Remove warning banner conditionals.
    @Test
    fun warningBanner() {
        val context = { version: String ->
            testClientCodegenContext(
                model = """
                    namespace test

                    service Foobaz {
                    }
                """.asSmithyModel(),
                settings = testClientRustSettings(
                    moduleVersion = version,
                    service = ShapeId.from("test#Foobaz"),
                    runtimeConfig = AwsTestRuntimeConfig,
                    customizationConfig =
                    ObjectNode.parse(
                        """
                        { "awsSdk": {
                            "awsConfigVersion": "dontcare" } }
                        """,
                    ) as ObjectNode,
                ),
            )
        }

        // Test unstable versions first
        var codegenContext = context("0.36.0")
        var result = AwsCrateDocGenerator(codegenContext).docText(includeHeader = false, includeLicense = false, asComments = true).let { writable ->
            val writer = RustWriter.root()
            writable(writer)
            writer.toString()
        }
        assertTrue(result.contains("The SDK is currently released as a developer preview"))

        // And now stable versions
        codegenContext = context("1.0.0")
        result = AwsCrateDocGenerator(codegenContext).docText(includeHeader = false, includeLicense = false, asComments = true).let { writable ->
            val writer = RustWriter.root()
            writable(writer)
            writer.toString()
        }
        assertFalse(result.contains("The SDK is currently released as a developer preview"))
    }
}
