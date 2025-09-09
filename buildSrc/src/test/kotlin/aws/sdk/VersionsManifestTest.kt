/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package aws.sdk

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Test

class VersionsManifestTest {
    @Test
    fun `it should parse versions toml`() {
        val manifest =
            VersionsManifest.fromString(
                """
                smithy_rs_revision = 'some-smithy-rs-revision'

                [crates.aws-config]
                category = 'AwsRuntime'
                version = '0.12.0'
                source_hash = '12d172094a2576e6f4d00a8ba58276c0d4abc4e241bb75f0d3de8ac3412e8e47'

                [crates.aws-sdk-account]
                category = 'AwsSdk'
                version = '0.12.0'
                source_hash = 'a0dfc080638b1d803745f0bd66b610131783cf40ab88fd710dce906fc69b983e'
                model_hash = '179bbfd915093dc3bec5444771da2b20d99a37d104ba25f0acac9aa0d5bb758a'
                """.trimIndent(),
            )

        assertEquals("some-smithy-rs-revision", manifest.smithyRsRevision)
        assertEquals(
            mapOf(
                "aws-config" to
                    CrateVersion(
                        category = "AwsRuntime",
                        version = "0.12.0",
                        sourceHash = "12d172094a2576e6f4d00a8ba58276c0d4abc4e241bb75f0d3de8ac3412e8e47",
                    ),
                "aws-sdk-account" to
                    CrateVersion(
                        category = "AwsSdk",
                        version = "0.12.0",
                        sourceHash = "a0dfc080638b1d803745f0bd66b610131783cf40ab88fd710dce906fc69b983e",
                        modelHash = "179bbfd915093dc3bec5444771da2b20d99a37d104ba25f0acac9aa0d5bb758a",
                    ),
            ),
            manifest.crates,
        )
    }
}
