/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package aws.sdk

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Test

class ModelMetadataTest {
    @Test
    fun `it should parse an empty file`() {
        val result = ModelMetadata.fromString("")
        assertFalse(result.hasCrates())
    }

    @Test
    fun `it should parse`() {
        val contents = """
            [crates.aws-sdk-someservice]
            kind = "Feature"

            [crates.aws-sdk-s3]
            kind = "Documentation"
        """.trimIndent()

        val result = ModelMetadata.fromString(contents)
        assertEquals(ChangeType.FEATURE, result.changeType("aws-sdk-someservice"))
        assertEquals(ChangeType.DOCUMENTATION, result.changeType("aws-sdk-s3"))
    }
}
