/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package aws.sdk

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Test

class IndependentCrateVersionerTest {
    @Test
    fun devPreviewSmithyRsChanged() {
        val versioner = IndependentCrateVersioner(
            VersionsManifest(
                smithyRsRevision = "smithy-rs-1",
                awsDocSdkExamplesRevision = "dontcare",
                crates = mapOf(
                    "aws-sdk-dynamodb" to CrateVersion(
                        category = "AwsSdk",
                        version = "0.11.3"
                    ),
                    "aws-sdk-ec2" to CrateVersion(
                        category = "AwsSdk",
                        version = "0.10.1"
                    ),
                    "aws-sdk-s3" to CrateVersion(
                        category = "AwsSdk",
                        version = "0.12.0"
                    )
                )
            ),
            ModelMetadata(
                crates = mapOf(
                    "aws-sdk-dynamodb" to ChangeType.FEATURE,
                    "aws-sdk-ec2" to ChangeType.DOCUMENTATION
                )
            ),
            devPreview = true,
            smithyRsVersion = "smithy-rs-2"
        )

        // The code generator changed, so all minor versions should bump
        assertEquals("0.12.0", versioner.decideCrateVersion("aws-sdk-dynamodb"))
        assertEquals("0.11.0", versioner.decideCrateVersion("aws-sdk-ec2"))
        assertEquals("0.13.0", versioner.decideCrateVersion("aws-sdk-s3"))
        assertEquals("0.1.0", versioner.decideCrateVersion("aws-sdk-somenewservice"))
    }

    @Test
    fun devPreviewSameCodeGenerator() {
        val versioner = IndependentCrateVersioner(
            VersionsManifest(
                smithyRsRevision = "smithy-rs-1",
                awsDocSdkExamplesRevision = "dontcare",
                crates = mapOf(
                    "aws-sdk-dynamodb" to CrateVersion(
                        category = "AwsSdk",
                        version = "0.11.3"
                    ),
                    "aws-sdk-ec2" to CrateVersion(
                        category = "AwsSdk",
                        version = "0.10.1"
                    ),
                    "aws-sdk-s3" to CrateVersion(
                        category = "AwsSdk",
                        version = "0.12.0"
                    )
                )
            ),
            ModelMetadata(
                crates = mapOf(
                    "aws-sdk-dynamodb" to ChangeType.FEATURE,
                    "aws-sdk-ec2" to ChangeType.DOCUMENTATION
                )
            ),
            devPreview = true,
            smithyRsVersion = "smithy-rs-1"
        )

        assertEquals("0.11.4", versioner.decideCrateVersion("aws-sdk-dynamodb"))
        assertEquals("0.10.2", versioner.decideCrateVersion("aws-sdk-ec2"))
        assertEquals("0.12.0", versioner.decideCrateVersion("aws-sdk-s3"))
        assertEquals("0.1.0", versioner.decideCrateVersion("aws-sdk-somenewservice"))
    }

    @Test
    fun smithyRsChanged() {
        val versioner = IndependentCrateVersioner(
            VersionsManifest(
                smithyRsRevision = "smithy-rs-1",
                awsDocSdkExamplesRevision = "dontcare",
                crates = mapOf(
                    "aws-sdk-dynamodb" to CrateVersion(
                        category = "AwsSdk",
                        version = "1.11.3"
                    ),
                    "aws-sdk-ec2" to CrateVersion(
                        category = "AwsSdk",
                        version = "1.10.1"
                    ),
                    "aws-sdk-s3" to CrateVersion(
                        category = "AwsSdk",
                        version = "1.12.0"
                    )
                )
            ),
            ModelMetadata(
                crates = mapOf(
                    "aws-sdk-dynamodb" to ChangeType.FEATURE,
                    "aws-sdk-ec2" to ChangeType.DOCUMENTATION
                )
            ),
            devPreview = false,
            smithyRsVersion = "smithy-rs-2"
        )

        // The code generator changed, so all minor versions should bump
        assertEquals("1.12.0", versioner.decideCrateVersion("aws-sdk-dynamodb"))
        assertEquals("1.11.0", versioner.decideCrateVersion("aws-sdk-ec2"))
        assertEquals("1.13.0", versioner.decideCrateVersion("aws-sdk-s3"))
        assertEquals("1.0.0", versioner.decideCrateVersion("aws-sdk-somenewservice"))
    }

    @Test
    fun sameCodeGenerator() {
        val versioner = IndependentCrateVersioner(
            VersionsManifest(
                smithyRsRevision = "smithy-rs-1",
                awsDocSdkExamplesRevision = "dontcare",
                crates = mapOf(
                    "aws-sdk-dynamodb" to CrateVersion(
                        category = "AwsSdk",
                        version = "1.11.3"
                    ),
                    "aws-sdk-ec2" to CrateVersion(
                        category = "AwsSdk",
                        version = "1.10.1"
                    ),
                    "aws-sdk-s3" to CrateVersion(
                        category = "AwsSdk",
                        version = "1.12.0"
                    )
                )
            ),
            ModelMetadata(
                crates = mapOf(
                    "aws-sdk-dynamodb" to ChangeType.FEATURE,
                    "aws-sdk-ec2" to ChangeType.DOCUMENTATION
                )
            ),
            devPreview = false,
            smithyRsVersion = "smithy-rs-1"
        )

        assertEquals("1.12.0", versioner.decideCrateVersion("aws-sdk-dynamodb"))
        assertEquals("1.10.2", versioner.decideCrateVersion("aws-sdk-ec2"))
        assertEquals("1.12.0", versioner.decideCrateVersion("aws-sdk-s3"))
        assertEquals("1.0.0", versioner.decideCrateVersion("aws-sdk-somenewservice"))
    }
}
