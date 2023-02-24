/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.util

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test

class DocManipulatorTest {
    @Test
    fun `test truncateDocsAfterFirstPeriod`() {
        DocManipulator.truncateDocsAfterFirstPeriod(
            "<p>Don't truncate.</p>",
        ) shouldBe "<p>Don't truncate.</p>"

        DocManipulator.truncateDocsAfterFirstPeriod(
            "<p>Please truncate. My content.</p>",
        ) shouldBe "<p>Please truncate.</p>"

        DocManipulator.truncateDocsAfterFirstPeriod(
            "<p><code>Thing</code> is a thing. Use it to do X, Y, and Z.</p>",
        ) shouldBe "<p><code>Thing</code> is a thing.</p>"

        DocManipulator.truncateDocsAfterFirstPeriod(
            "<p>Use with <code>foo.baz</code> to do a thing. Use it to do X, Y, and Z.</p>",
        ) shouldBe "<p>Use with <code>foo.baz</code> to do a thing.</p>"

        DocManipulator.truncateDocsAfterFirstPeriod(
            "<p>Use with <code>foo.baz</code> to do a thing. Use it to do X, Y, and Z.</p><p>Some other text</p>",
        ) shouldBe "<p>Use with <code>foo.baz</code> to do a thing.</p>"

        DocManipulator.truncateDocsAfterFirstPeriod(
            "<p>Use with <code>foo.baz</code> to do a <b>thing. Use it to do X, Y,</b> and Z.</p>",
        ) shouldBe "<p>Use with <code>foo.baz</code> to do a <b>thing.</b></p>"

        // real world example
        DocManipulator.truncateDocsAfterFirstPeriod(
            """
            <p>The identifier of the Key Management Service (KMS) KMS key to use for Amazon EBS encryption.
            If this parameter is not specified, your KMS key for Amazon EBS is used.
            If <code>KmsKeyId</code> is specified, the encrypted state must be <code>true</code>.</p>
            <p>You can specify the KMS key using any of the following:</p>
            <ul>
            <li><p>Key ID. For example, 1234abcd-12ab-34cd-56ef-1234567890ab.</p></li>
            <li><p>Key alias. For example, alias/ExampleAlias.</p></li>
            <li><p>Key ARN. For example, arn:aws:kms:us-east-1:012345678910:key/1234abcd-12ab-34cd-56ef-1234567890ab.</p></li>
            <li><p>Alias ARN. For example, arn:aws:kms:us-east-1:012345678910:alias/ExampleAlias.</p></li>
            </ul>
            <p>Amazon Web Services authenticates the KMS key asynchronously. Therefore, if you specify an ID, alias,
            or ARN that is not valid, the action can appear to complete, but eventually fails.</p>
            """.trimIndent(),
        ) shouldBe "<p>The identifier of the Key Management Service (KMS) KMS key to use for Amazon EBS encryption.</p>"
    }
}
